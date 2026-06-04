use std::collections::VecDeque;
use std::sync::atomic::AtomicU32;
use std::sync::Arc;

use deep_filter::tract::{DfParams, DfTract, RuntimeParams};
use ndarray::Array2;

use crate::audio::graph::NoiseSuppressorData;
use crate::audio::resample::StereoResampler;

use super::util::load_f32;
use super::{Effect, EffectControl};

const MODEL_SR: u32 = 48_000;
// DfTract is mono-only here (df_states hardcoded to len 1); stereo corrupts the
// signal proportionally to attenuation. Downmix in, fan mono back out to L/R.
const CHANNELS: usize = 1;
const DOWN_CHUNK: usize = 512;
const CAP: usize = 8192;

#[derive(Clone)]
pub struct NoiseSuppressorControls {
    pub atten_lim_db: Arc<AtomicU32>,
    pub pf_beta: Arc<AtomicU32>,
    pub min_thresh_db: Arc<AtomicU32>,
    pub max_erb_thresh_db: Arc<AtomicU32>,
    pub max_df_thresh_db: Arc<AtomicU32>,
}

pub struct NoiseSuppressorEffect {
    ctl: NoiseSuppressorControls,
    state: Option<ModelState>,
    last: Params,
}

#[derive(Clone, Copy, PartialEq)]
struct Params {
    atten: f32,
    pf_beta: f32,
    min_thresh: f32,
    max_erb: f32,
    max_df: f32,
}

impl Params {
    fn load(c: &NoiseSuppressorControls) -> Self {
        Self {
            atten: load_f32(&c.atten_lim_db),
            pf_beta: load_f32(&c.pf_beta),
            min_thresh: load_f32(&c.min_thresh_db),
            max_erb: load_f32(&c.max_erb_thresh_db),
            max_df: load_f32(&c.max_df_thresh_db),
        }
    }
}

// DfTract holds Rc, so it is !Send. Its graph is only ever moved by exclusive
// ownership (SPSC ring), never shared across threads.
struct SendModel(DfTract);
unsafe impl Send for SendModel {}

// Resamples the output-rate signal to 48k for the model and back. Present only
// when the output rate isn't already 48k.
struct Resample {
    down: StereoResampler,
    up: StereoResampler,
    down_in: VecDeque<f32>,
    up_in: VecDeque<f32>,
    chunk: Vec<f32>,
    scratch: Vec<f32>,
}

impl Resample {
    fn to_model(&mut self, block: &[f32], dst: &mut Vec<f32>) {
        self.down_in.extend(block.iter().copied());
        while self.down_in.len() >= DOWN_CHUNK * 2 {
            self.chunk.clear();
            for _ in 0..DOWN_CHUNK * 2 {
                self.chunk.push(self.down_in.pop_front().unwrap());
            }
            let _ = self.down.process_chunk(&self.chunk, dst);
        }
    }

    fn from_model(&mut self, enh48: &[f32], hop: usize, dst: &mut VecDeque<f32>) {
        self.up_in.extend(enh48.iter().copied());
        while self.up_in.len() >= hop * 2 {
            self.chunk.clear();
            for _ in 0..hop * 2 {
                self.chunk.push(self.up_in.pop_front().unwrap());
            }
            self.scratch.clear();
            let _ = self.up.process_chunk(&self.chunk, &mut self.scratch);
            dst.extend(self.scratch.iter().copied());
        }
    }
}

struct ModelState {
    model: SendModel,
    hop: usize,
    latency: usize,
    in_mono: VecDeque<f32>,
    noisy: Array2<f32>,
    enh: Array2<f32>,
    resample: Option<Resample>,
    mid48: Vec<f32>,
    enh48: Vec<f32>,
    out: VecDeque<f32>,
}

impl NoiseSuppressorEffect {
    pub fn new(d: NoiseSuppressorData, sample_rate: u32) -> (Self, EffectControl) {
        let ctl = NoiseSuppressorControls {
            atten_lim_db: Arc::new(AtomicU32::new(d.attenuation_limit_db.to_bits())),
            pf_beta: Arc::new(AtomicU32::new(d.post_filter_beta.max(0.0).to_bits())),
            min_thresh_db: Arc::new(AtomicU32::new(d.min_thresh_db.to_bits())),
            max_erb_thresh_db: Arc::new(AtomicU32::new(d.max_erb_thresh_db.to_bits())),
            max_df_thresh_db: Arc::new(AtomicU32::new(d.max_df_thresh_db.to_bits())),
        };
        let control = EffectControl::NoiseSuppressor { controls: ctl.clone() };
        (Self::from_state(ctl, sample_rate), control)
    }

    pub fn from_state(ctl: NoiseSuppressorControls, sample_rate: u32) -> Self {
        let initial = Params::load(&ctl);
        Self {
            ctl,
            state: ModelState::build(initial, sample_rate),
            last: initial,
        }
    }
}

impl ModelState {
    fn build(p: Params, output_sr: u32) -> Option<Self> {
        let mut rp = RuntimeParams::default_with_ch(CHANNELS);
        rp.atten_lim_db = p.atten;
        rp.post_filter_beta = p.pf_beta;
        rp.post_filter = p.pf_beta > 0.0;
        rp.min_db_thresh = p.min_thresh;
        rp.max_db_erb_thresh = p.max_erb;
        rp.max_db_df_thresh = p.max_df;
        let model = match DfTract::new(DfParams::default(), &rp) {
            Ok(m) => m,
            Err(e) => {
                tracing::error!("NoiseSuppressor model init failed: {e:#}");
                return None;
            }
        };
        let hop = model.hop_size;
        let lookahead = model.lookahead;

        let resample = if output_sr == MODEL_SR {
            None
        } else {
            let down = StereoResampler::new(output_sr, MODEL_SR, DOWN_CHUNK);
            let up = StereoResampler::new(MODEL_SR, output_sr, hop);
            match (down, up) {
                (Ok(down), Ok(up)) => Some(Resample {
                    down,
                    up,
                    down_in: VecDeque::with_capacity(CAP),
                    up_in: VecDeque::with_capacity(CAP),
                    chunk: Vec::with_capacity(DOWN_CHUNK * 2),
                    scratch: Vec::with_capacity(CAP),
                }),
                (down, up) => {
                    let e = down.err().or(up.err()).unwrap();
                    tracing::error!("NoiseSuppressor resampler init failed: {e}");
                    return None;
                }
            }
        };

        let prime = if resample.is_some() { hop + DOWN_CHUNK } else { hop };
        let mut out = VecDeque::with_capacity(CAP);
        // Prime so a block read never outruns the hop-aligned producer.
        out.extend(std::iter::repeat(0.0).take(prime * 2));

        Some(Self {
            model: SendModel(model),
            hop,
            latency: prime + lookahead,
            in_mono: VecDeque::with_capacity(CAP),
            noisy: Array2::zeros((CHANNELS, hop)),
            enh: Array2::zeros((CHANNELS, hop)),
            resample,
            mid48: Vec::with_capacity(CAP),
            enh48: Vec::with_capacity(CAP),
            out,
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

        let now = Params::load(&self.ctl);
        if now != self.last {
            if now.atten != self.last.atten {
                s.model.0.set_atten_lim(now.atten);
            }
            if now.pf_beta != self.last.pf_beta {
                s.model.0.set_pf_beta(now.pf_beta);
            }
            s.model.0.min_db_thresh = now.min_thresh;
            s.model.0.max_db_erb_thresh = now.max_erb;
            s.model.0.max_db_df_thresh = now.max_df;
            self.last = now;
        }

        let stereo = &mut samples[..frames * 2];

        s.mid48.clear();
        match s.resample.as_mut() {
            None => s.mid48.extend_from_slice(stereo),
            Some(r) => r.to_model(stereo, &mut s.mid48),
        }

        s.enh48.clear();
        for f in s.mid48.chunks_exact(2) {
            s.in_mono.push_back(0.5 * (f[0] + f[1]));
        }
        while s.in_mono.len() >= s.hop {
            for i in 0..s.hop {
                s.noisy[[0, i]] = s.in_mono.pop_front().unwrap();
            }
            let _ = s.model.0.process(s.noisy.view(), s.enh.view_mut());
            for i in 0..s.hop {
                let m = s.enh[[0, i]];
                s.enh48.push(m);
                s.enh48.push(m);
            }
        }

        match s.resample.as_mut() {
            None => s.out.extend(s.enh48.iter().copied()),
            Some(r) => r.from_model(&s.enh48, s.hop, &mut s.out),
        }

        for f in stereo.chunks_exact_mut(2) {
            f[0] = s.out.pop_front().unwrap_or(0.0);
            f[1] = s.out.pop_front().unwrap_or(0.0);
        }
    }

    fn latency_frames(&self) -> usize {
        self.state.as_ref().map_or(0, |s| s.latency)
    }
}
