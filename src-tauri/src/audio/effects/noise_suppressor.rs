use std::collections::VecDeque;
use std::sync::atomic::AtomicU32;
use std::sync::Arc;

use deep_filter::tract::{DfParams, DfTract, RuntimeParams};
use ndarray::Array2;

use crate::audio::graph::NoiseSuppressorData;

use super::util::{load_f32, store_f32};
use super::{Effect, EffectControl};

const MODEL_SR: u32 = 48_000;
const CHANNELS: usize = 2;

pub struct NoiseSuppressorEffect {
    atten_lim_db: Arc<AtomicU32>,
    state: Option<ModelState>,
    last_atten: f32,
}

// DfTract holds Rc, so it is !Send. Its graph is only ever moved by exclusive
// ownership (SPSC ring), never shared across threads.
struct SendModel(DfTract);
unsafe impl Send for SendModel {}

struct ModelState {
    model: SendModel,
    hop: usize,
    latency: usize,
    in_l: VecDeque<f32>,
    in_r: VecDeque<f32>,
    out_l: VecDeque<f32>,
    out_r: VecDeque<f32>,
    noisy: Array2<f32>,
    enh: Array2<f32>,
}

impl NoiseSuppressorEffect {
    pub fn new(d: NoiseSuppressorData, sample_rate: u32) -> (Self, EffectControl) {
        let atten_lim_db = Arc::new(AtomicU32::new(d.attenuation_limit_db.to_bits()));
        store_f32(&atten_lim_db, d.attenuation_limit_db);
        let control = EffectControl::NoiseSuppressor {
            attenuation_limit_db: atten_lim_db.clone(),
        };
        (Self::from_state(atten_lim_db, sample_rate), control)
    }

    pub fn from_state(atten_lim_db: Arc<AtomicU32>, sample_rate: u32) -> Self {
        let initial = load_f32(&atten_lim_db);
        let state = if sample_rate == MODEL_SR {
            ModelState::build(initial)
        } else {
            tracing::warn!(
                sample_rate,
                "NoiseSuppressor requires 48 kHz; passing audio through unchanged"
            );
            None
        };
        Self {
            atten_lim_db,
            state,
            last_atten: initial,
        }
    }
}

impl ModelState {
    fn build(initial_atten_db: f32) -> Option<Self> {
        let mut rp = RuntimeParams::default_with_ch(CHANNELS);
        rp.atten_lim_db = initial_atten_db;
        let model = match DfTract::new(DfParams::default(), &rp) {
            Ok(m) => m,
            Err(e) => {
                tracing::error!("NoiseSuppressor model init failed: {e:#}");
                return None;
            }
        };
        let hop = model.hop_size;
        let latency = hop + model.lookahead;
        let cap = hop + 2048;
        let mut out_l = VecDeque::with_capacity(cap);
        let mut out_r = VecDeque::with_capacity(cap);
        // Prime one hop so a block read never outruns the hop-aligned producer.
        out_l.extend(std::iter::repeat(0.0).take(hop));
        out_r.extend(std::iter::repeat(0.0).take(hop));
        Some(Self {
            model: SendModel(model),
            hop,
            latency,
            in_l: VecDeque::with_capacity(cap),
            in_r: VecDeque::with_capacity(cap),
            out_l,
            out_r,
            noisy: Array2::zeros((CHANNELS, hop)),
            enh: Array2::zeros((CHANNELS, hop)),
        })
    }
}

impl Effect for NoiseSuppressorEffect {
    fn process(&mut self, samples: &mut [f32], frames: usize) {
        if frames == 0 {
            return;
        }
        let Some(s) = self.state.as_mut() else {
            return;
        };

        let target = load_f32(&self.atten_lim_db);
        if target != self.last_atten {
            s.model.0.set_atten_lim(target);
            self.last_atten = target;
        }

        let stereo = &mut samples[..frames * 2];
        for f in stereo.chunks_exact(2) {
            s.in_l.push_back(f[0]);
            s.in_r.push_back(f[1]);
        }

        while s.in_l.len() >= s.hop {
            for i in 0..s.hop {
                s.noisy[[0, i]] = s.in_l.pop_front().unwrap();
                s.noisy[[1, i]] = s.in_r.pop_front().unwrap();
            }
            let _ = s.model.0.process(s.noisy.view(), s.enh.view_mut());
            for i in 0..s.hop {
                s.out_l.push_back(s.enh[[0, i]]);
                s.out_r.push_back(s.enh[[1, i]]);
            }
        }

        for f in stereo.chunks_exact_mut(2) {
            f[0] = s.out_l.pop_front().unwrap_or(0.0);
            f[1] = s.out_r.pop_front().unwrap_or(0.0);
        }
    }

    fn latency_frames(&self) -> usize {
        self.state.as_ref().map_or(0, |s| s.latency)
    }
}
