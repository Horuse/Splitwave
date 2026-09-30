use std::collections::VecDeque;
use std::sync::atomic::AtomicU32;
use std::sync::Arc;

#[cfg(not(all(windows, target_arch = "aarch64")))]
mod model {
    pub use deep_filter::tract::{DfParams, DfTract, RuntimeParams};
}

// DeepFilterNet is excluded from Windows ARM64 (see Cargo.toml): tract's arm64
// kernels are GNU-as only and can't assemble to COFF. DfTract::new always
// errors here, so the effect builds no model and runs as passthrough.
#[cfg(all(windows, target_arch = "aarch64"))]
mod model {
    use ndarray::{ArrayView2, ArrayViewMut2};

    pub struct DfParams;
    impl DfParams {
        pub fn default() -> Self {
            DfParams
        }
    }

    pub struct RuntimeParams {
        pub atten_lim_db: f32,
        pub post_filter_beta: f32,
        pub post_filter: bool,
        pub min_db_thresh: f32,
        pub max_db_erb_thresh: f32,
        pub max_db_df_thresh: f32,
    }
    impl RuntimeParams {
        pub fn default_with_ch(_ch: usize) -> Self {
            Self {
                atten_lim_db: 0.0,
                post_filter_beta: 0.0,
                post_filter: false,
                min_db_thresh: 0.0,
                max_db_erb_thresh: 0.0,
                max_db_df_thresh: 0.0,
            }
        }
    }

    #[derive(Debug)]
    pub struct Unsupported;
    impl std::fmt::Display for Unsupported {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "DeepFilterNet unavailable on Windows ARM64")
        }
    }
    impl std::error::Error for Unsupported {}

    pub struct DfTract {
        pub hop_size: usize,
        pub fft_size: usize,
        pub conv_lookahead: usize,
        pub min_db_thresh: f32,
        pub max_db_erb_thresh: f32,
        pub max_db_df_thresh: f32,
    }
    impl DfTract {
        pub fn new(_p: DfParams, _r: &RuntimeParams) -> Result<Self, Unsupported> {
            Err(Unsupported)
        }
        pub fn set_atten_lim(&mut self, _v: f32) {}
        pub fn set_pf_beta(&mut self, _v: f32) {}
        pub fn process(&mut self, _n: ArrayView2<f32>, _e: ArrayViewMut2<f32>) {}
    }
}

use model::{DfParams, DfTract, RuntimeParams};
use ndarray::Array2;

use crate::audio::graph::NoiseSuppressorData;
use crate::audio::graph::MAX_BUFFER_FRAMES;
use crate::audio::resample::{MultiResampler, MultiResamplerOut};

use super::offload::{BlockProcessor, Offload};
use super::util::load_f32;
use super::{Effect, EffectControl};

const MODEL_SR: u32 = 48_000;
// DfTract is mono-only here (df_states hardcoded to len 1); stereo corrupts the
// signal proportionally to attenuation. Downmix in, fan mono back out to L/R.
const CHANNELS: usize = 1;
/// Slack each resampler's +-1 frame of jitter per hop needs, on the way in and
/// on the way out.
const RESAMPLE_SLACK: usize = 4;
/// DeepFilterNet treats a hop whose mean square is under this as silence: it
/// writes zeros and does not step its buffers, so what they held comes out at
/// the next louder hop, shifted in time.
const DF_SILENCE_GATE: f32 = 1e-7;
/// Below this limit DeepFilterNet passes the input through undelayed, which
/// would move the output by the model's latency whenever the limit crossed it.
const MIN_ATTEN_DB: f32 = 0.01;

#[derive(Clone)]
pub struct NoiseSuppressorControls {
    pub atten_lim_db: Arc<AtomicU32>,
    pub pf_beta: Arc<AtomicU32>,
    pub min_thresh_db: Arc<AtomicU32>,
    pub max_erb_thresh_db: Arc<AtomicU32>,
    pub max_df_thresh_db: Arc<AtomicU32>,
}

pub struct NoiseSuppressorEffect {
    backend: Option<Backend>,
    latency: usize,
    /// The model's hop in output frames, when it exceeds the engine block.
    working_block: Option<usize>,
}

enum Backend {
    Offloaded(Offload),
    // An offline render outruns the offload thread and would read back silence.
    Inline { worker: ModelWorker, out: Vec<f32> },
}

struct ModelWorker {
    ctl: NoiseSuppressorControls,
    state: ModelState,
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

// DfTract holds Rc, so it is !Send. Ownership transfers to the offload thread
// once and stays there; it is never shared.
struct SendModel(DfTract);
unsafe impl Send for SendModel {}

// Resamples the output-rate signal to 48k for the model and back, a hop at a
// time. Present only when the output rate isn't already 48k.
struct Resample {
    /// Exactly one model hop out per call; its input need wanders by a frame.
    down: MultiResamplerOut,
    /// One model hop in per call; its output wanders by a frame.
    up: MultiResampler,
    chunk: Vec<f32>,
}

struct ModelState {
    model: SendModel,
    /// The model's hop at 48 kHz.
    hop: usize,
    /// Output frames this state delays by: the model's own latency, the
    /// resamplers', and the slack queued ahead on either side.
    latency: usize,
    /// The model hop in output frames: what one call normally carries.
    hop_out: usize,
    /// Interleaved stereo at the output rate, waiting to fill a hop.
    in_q: VecDeque<f32>,
    noisy: Array2<f32>,
    enh: Array2<f32>,
    resample: Option<Resample>,
    /// One model hop, interleaved stereo at 48 kHz.
    mid48: Vec<f32>,
    enh48: Vec<f32>,
    out: VecDeque<f32>,
    /// Output samples asked for before they were ready.
    #[cfg(test)]
    short: usize,
}

impl NoiseSuppressorEffect {
    pub fn new(
        d: NoiseSuppressorData,
        sample_rate: u32,
        block_frames: usize,
        realtime: bool,
    ) -> (Self, EffectControl) {
        let ctl = NoiseSuppressorControls {
            atten_lim_db: Arc::new(AtomicU32::new(d.attenuation_limit_db.to_bits())),
            pf_beta: Arc::new(AtomicU32::new(d.post_filter_beta.max(0.0).to_bits())),
            min_thresh_db: Arc::new(AtomicU32::new(d.min_thresh_db.to_bits())),
            max_erb_thresh_db: Arc::new(AtomicU32::new(d.max_erb_thresh_db.to_bits())),
            max_df_thresh_db: Arc::new(AtomicU32::new(d.max_df_thresh_db.to_bits())),
        };
        let control = EffectControl::NoiseSuppressor {
            controls: ctl.clone(),
        };
        (
            Self::from_state(ctl, sample_rate, block_frames, realtime),
            control,
        )
    }

    pub fn from_state(
        ctl: NoiseSuppressorControls,
        sample_rate: u32,
        block_frames: usize,
        realtime: bool,
    ) -> Self {
        let initial = Params::load(&ctl);
        let Some(state) = ModelState::build(initial, sample_rate, realtime) else {
            return Self {
                backend: None,
                latency: 0,
                working_block: None,
            };
        };
        let hop_out = state.hop_out;
        let working_block = (hop_out > block_frames).then_some(hop_out);
        let worker = ModelWorker {
            ctl,
            state,
            last: initial,
        };
        if !realtime {
            let out = Vec::with_capacity(MAX_BUFFER_FRAMES * 2);
            return Self {
                latency: worker.state.latency,
                backend: Some(Backend::Inline { worker, out }),
                working_block,
            };
        }
        let model_latency = worker.state.latency;
        // The offload hands the model exactly one hop at a time.
        match Offload::spawn(
            "noise_suppressor",
            worker,
            2,
            Some(hop_out),
            block_frames,
            sample_rate,
        ) {
            Ok(offload) => Self {
                latency: model_latency + offload.latency_frames(),
                backend: Some(Backend::Offloaded(offload)),
                working_block,
            },
            Err(mut worker) => {
                worker.state.feed_any_size();
                let out = Vec::with_capacity(MAX_BUFFER_FRAMES * 2);
                Self {
                    latency: worker.state.latency,
                    backend: Some(Backend::Inline { worker, out }),
                    working_block,
                }
            }
        }
    }

    /// The model's hop, the smallest step it can answer in, when larger
    /// than the engine block.
    pub fn working_block(&self) -> Option<usize> {
        self.working_block
    }
}
impl ModelState {
    /// `hop_fed`: every call carries exactly one hop of output frames, as the
    /// offload delivers them. Otherwise calls come in any size and a hop of
    /// output is queued ahead to cover the one still being gathered.
    fn build(p: Params, output_sr: u32, hop_fed: bool) -> Option<Self> {
        let mut rp = RuntimeParams::default_with_ch(CHANNELS);
        rp.atten_lim_db = p.atten.max(MIN_ATTEN_DB);
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
        // Overlap-add holds back a window less a hop, and the model answers
        // for the frame `conv_lookahead` hops behind the newest.
        let model_delay48 = model.fft_size - hop + model.conv_lookahead * hop;
        let to_out = |frames48: usize| {
            (frames48 as u64 * output_sr as u64).div_ceil(MODEL_SR as u64) as usize
        };
        let hop_out = to_out(hop);
        let exact = (hop as u64 * output_sr as u64).is_multiple_of(MODEL_SR as u64);

        // Both resamplers align their output to their input, so their filters
        // show up only as the queues below: the way in reads a filter's
        // half-length ahead on its first hop, the way out holds that much
        // back on its first.
        let (resample, preroll, queued) = if output_sr == MODEL_SR {
            (None, 0, 0)
        } else {
            let down = MultiResamplerOut::new(output_sr, MODEL_SR, hop, 2);
            let up = MultiResampler::new(MODEL_SR, output_sr, hop, 2);
            let (down, up) = match (down, up) {
                (Ok(down), Ok(up)) => (down, up),
                (down, up) => {
                    let e = down.err().or(up.err()).unwrap();
                    tracing::error!("NoiseSuppressor resampler init failed: {e}");
                    return None;
                }
            };
            let preroll = down.input_frames_next().saturating_sub(hop_out) + RESAMPLE_SLACK;
            let queued = up.delay_frames() + 2 * RESAMPLE_SLACK;
            let chunk = Vec::with_capacity(down.input_frames_max() * 2);
            (Some(Resample { down, up, chunk }), preroll, queued)
        };
        let mut queued = queued;
        // A rate whose hop is not a whole number of frames steps a hop more
        // or less per call now and then, as does a caller of any block size.
        if !hop_fed || !exact {
            queued += hop_out;
        }

        // Either queue holds its prefill, a hop being gathered or answered,
        // and a whole engine block on top: sized so the audio thread never
        // grows them.
        let cap = (preroll + queued + 2 * hop_out + MAX_BUFFER_FRAMES) * 2;
        let mut in_q = VecDeque::with_capacity(cap);
        in_q.extend(std::iter::repeat_n(0.0, preroll * 2));
        let mut out = VecDeque::with_capacity(cap);
        out.extend(std::iter::repeat_n(0.0, queued * 2));

        Some(Self {
            model: SendModel(model),
            hop,
            latency: to_out(model_delay48) + preroll + queued,
            hop_out,
            in_q,
            noisy: Array2::zeros((CHANNELS, hop)),
            enh: Array2::zeros((CHANNELS, hop)),
            resample,
            mid48: Vec::with_capacity(hop * 2),
            enh48: Vec::with_capacity(hop * 2),
            out,
            #[cfg(test)]
            short: 0,
        })
    }

    /// The offload could not start: calls now come in engine blocks.
    fn feed_any_size(&mut self) {
        self.out.extend(std::iter::repeat_n(0.0, self.hop_out * 2));
        self.latency += self.hop_out;
    }

    /// Runs every whole hop `in_q` holds.
    fn step(&mut self) {
        loop {
            let need = self
                .resample
                .as_ref()
                .map_or(self.hop, |r| r.down.input_frames_next());
            if self.in_q.len() < need * 2 {
                return;
            }
            self.mid48.clear();
            match self.resample.as_mut() {
                None => self.mid48.extend(self.in_q.drain(..need * 2)),
                Some(r) => {
                    r.chunk.clear();
                    r.chunk.extend(self.in_q.drain(..need * 2));
                    if r.down.process(&r.chunk, &mut self.mid48).is_err() {
                        self.mid48.clear();
                        self.mid48.resize(self.hop * 2, 0.0);
                    }
                }
            }
            self.run_model();
            match self.resample.as_mut() {
                None => self.out.extend(self.enh48.iter().copied()),
                Some(r) => {
                    r.chunk.clear();
                    if r.up.process_chunk(&self.enh48, &mut r.chunk).is_err() {
                        r.chunk.resize(self.hop_out * 2, 0.0);
                    }
                    self.out.extend(r.chunk.iter().copied());
                }
            }
        }
    }

    /// One hop of `mid48` through the model into `enh48`. A hop under the
    /// model's silence gate is raised just over it and the result lowered
    /// back, so the model steps its buffers on every hop; digital silence
    /// gets a dither far below hearing first, since no gain lifts zero.
    fn run_model(&mut self) {
        let mut power = 0.0f32;
        for (i, f) in self.mid48.chunks_exact(2).enumerate() {
            let m = 0.5 * (f[0] + f[1]);
            self.noisy[[0, i]] = m;
            power += m * m;
        }
        let mut gain = 1.0f32;
        if power / (self.hop as f32) < DF_SILENCE_GATE {
            for i in 0..self.hop {
                let dither = if i % 2 == 0 { 1e-7 } else { -1e-7 };
                self.noisy[[0, i]] += dither;
            }
            let power: f32 = self.noisy.iter().map(|x| x * x).sum();
            gain = (2.0 * DF_SILENCE_GATE * self.hop as f32 / power).sqrt();
            self.noisy.mapv_inplace(|x| x * gain);
        }
        let _ = self.model.0.process(self.noisy.view(), self.enh.view_mut());
        self.enh48.clear();
        for i in 0..self.hop {
            let m = self.enh[[0, i]] / gain;
            self.enh48.push(m);
            self.enh48.push(m);
        }
    }
}

impl BlockProcessor for ModelWorker {
    fn process(&mut self, input: &[f32], output: &mut Vec<f32>) {
        let now = Params::load(&self.ctl);
        let s = &mut self.state;
        if now != self.last {
            if now.atten != self.last.atten {
                s.model.0.set_atten_lim(now.atten.max(MIN_ATTEN_DB));
            }
            if now.pf_beta != self.last.pf_beta {
                s.model.0.set_pf_beta(now.pf_beta);
            }
            s.model.0.min_db_thresh = now.min_thresh;
            s.model.0.max_db_erb_thresh = now.max_erb;
            s.model.0.max_db_df_thresh = now.max_df;
            self.last = now;
        }
        s.in_q.extend(input.iter().copied());
        s.step();
        for _ in 0..input.len() {
            let v = s.out.pop_front();
            #[cfg(test)]
            if v.is_none() {
                s.short += 1;
            }
            output.push(v.unwrap_or(0.0));
        }
    }
}

impl Effect for NoiseSuppressorEffect {
    fn process(&mut self, samples: &mut [f32], frames: usize) {
        if frames == 0 {
            return;
        }
        match self.backend.as_mut() {
            Some(Backend::Offloaded(o)) => o.process(&mut samples[..frames * 2]),
            Some(Backend::Inline { worker, out }) => {
                out.clear();
                worker.process(&samples[..frames * 2], out);
                samples[..frames * 2].copy_from_slice(out);
            }
            None => {}
        }
    }

    fn latency_frames(&self) -> usize {
        self.latency
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::graph::NoiseSuppressorData;

    fn data() -> NoiseSuppressorData {
        NoiseSuppressorData {
            attenuation_limit_db: 15.0,
            post_filter_beta: 0.0,
            min_thresh_db: -10.0,
            max_erb_thresh_db: 30.0,
            max_df_thresh_db: 20.0,
            bypassed: false,
        }
    }

    #[test]
    fn inline_backend_processes_at_48k() {
        let (mut e, _) = NoiseSuppressorEffect::new(data(), 48_000, 1024, false);
        assert!(e.latency_frames() > 0, "model latency must be reported");
        for k in 0..4 {
            let mut buf: Vec<f32> = (0..512 * 2)
                .map(|i| {
                    let s = 0.3
                        * ((2.0 * std::f32::consts::PI * 440.0 * (k * 1024 + i / 2) as f32
                            / 48_000.0)
                            .sin());
                    [s, s]
                })
                .flatten()
                .collect();
            e.process(&mut buf, 512);
            assert!(buf.iter().all(|s| s.is_finite()), "block broke");
        }
    }

    #[test]
    fn inline_backend_resamples_at_44k1() {
        // Output rate ≠ 48 kHz exercises both resamplers.
        let (mut e, _) = NoiseSuppressorEffect::new(data(), 44_100, 1024, false);
        assert!(e.latency_frames() > 0);
        for _k in 0..4 {
            let mut buf = vec![0.2f32; 512 * 2];
            e.process(&mut buf, 512);
            assert!(buf.iter().all(|s| s.is_finite()), "block broke");
        }
    }

    #[test]
    fn live_param_changes_update_the_model() {
        let (mut e, c) = NoiseSuppressorEffect::new(data(), 48_000, 1024, false);
        let EffectControl::NoiseSuppressor { controls } = &c else {
            panic!("variant")
        };
        for _k in 0..2 {
            let mut buf = vec![0.2f32; 512 * 2];
            e.process(&mut buf, 512);
        }
        // Parameter change exercises the set_atten_lim / set_pf_beta branches.
        use std::sync::atomic::Ordering;
        controls
            .atten_lim_db
            .store(25.0f32.to_bits(), Ordering::Relaxed);
        controls.pf_beta.store(0.4f32.to_bits(), Ordering::Relaxed);
        let mut buf = vec![0.2f32; 512 * 2];
        e.process(&mut buf, 512);
        assert!(buf.iter().all(|s| s.is_finite()));
    }

    fn controls(atten_db: f32) -> NoiseSuppressorControls {
        let (_, c) = NoiseSuppressorEffect::new(
            NoiseSuppressorData {
                attenuation_limit_db: atten_db,
                ..data()
            },
            48_000,
            1024,
            false,
        );
        let EffectControl::NoiseSuppressor { controls } = c else {
            panic!("variant")
        };
        controls
    }

    /// A worker fed the way the offload feeds it: one hop of output frames
    /// per call.
    fn hop_fed(atten_db: f32, sample_rate: u32) -> ModelWorker {
        let ctl = controls(atten_db);
        let last = Params::load(&ctl);
        let state = ModelState::build(last, sample_rate, true).expect("model");
        ModelWorker { ctl, state, last }
    }

    fn noise(frames: usize) -> Vec<f32> {
        let mut x: u32 = 0x1234_5678;
        (0..frames)
            .flat_map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                let v = (x % 20_000) as f32 / 100_000.0 - 0.1;
                [v, v]
            })
            .collect()
    }

    /// The lag at which `out` matches `input` best, over `max_lag` frames.
    fn best_lag(input: &[f32], out: &[f32], max_lag: usize) -> usize {
        let frames = input.len() / 2;
        (0..max_lag)
            .max_by(|&a, &b| {
                let score = |lag: usize| -> f64 {
                    (lag..frames)
                        .map(|n| out[n * 2] as f64 * input[(n - lag) * 2] as f64)
                        .sum()
                };
                score(a).total_cmp(&score(b))
            })
            .unwrap()
    }

    #[test]
    fn the_declared_latency_is_the_real_delay() {
        for sample_rate in [48_000, 44_100] {
            for atten in [0.0, 6.0] {
                let case = format!("{sample_rate} Hz, {atten} dB");
                let input = noise(sample_rate as usize);

                // As the offload drives it.
                let mut w = hop_fed(atten, sample_rate);
                let hop = w.state.hop_out;
                let mut out = Vec::new();
                for chunk in input.chunks_exact(hop * 2) {
                    w.process(chunk, &mut out);
                }
                let fed = out.len();
                let lag = best_lag(&input[..fed], &out, 4_000);
                assert!(
                    lag.abs_diff(w.state.latency) <= 1,
                    "{case}, hop-fed: declared {}, measured {lag}",
                    w.state.latency
                );
                assert_eq!(w.state.short, 0, "{case}, hop-fed: output ran short");

                // Inline, in engine blocks of any size.
                for block in [64, 1024] {
                    let d = NoiseSuppressorData {
                        attenuation_limit_db: atten,
                        ..data()
                    };
                    let (mut e, _) = NoiseSuppressorEffect::new(d, sample_rate, block, false);
                    let mut out = input.clone();
                    for chunk in out.chunks_exact_mut(block * 2) {
                        e.process(chunk, block);
                    }
                    let n = input.len() / (block * 2) * block * 2;
                    let lag = best_lag(&input[..n], &out[..n], 4_000);
                    assert!(
                        lag.abs_diff(e.latency_frames()) <= 1,
                        "{case}, inline {block}: declared {}, measured {lag}",
                        e.latency_frames()
                    );
                }
            }
        }
    }

    #[test]
    fn a_hop_fed_worker_never_runs_short() {
        for sample_rate in [44_100, 48_000, 88_200, 96_000] {
            let mut w = hop_fed(15.0, sample_rate);
            let hop = w.state.hop_out;
            let input = vec![0.1f32; hop * 2];
            let mut out = Vec::new();
            for _ in 0..3_000 {
                out.clear();
                w.process(&input, &mut out);
            }
            assert_eq!(w.state.short, 0, "{sample_rate}: output ran short");
            assert!(
                w.state.in_q.len() < 2 * (hop + 2 * RESAMPLE_SLACK) * 2,
                "{sample_rate}: input piles up"
            );
        }
    }

    #[test]
    fn a_pause_brings_back_nothing_from_before_it() {
        // A burst, then silence the model would skip, then another burst.
        // Whatever sat in the model's buffers when the first burst ended
        // must come out right after it, not when the second one starts.
        let mut w = hop_fed(6.0, 48_000);
        let hop = w.state.hop_out;
        let tone = |n: usize| ((n % 100) as f32 / 100.0 * std::f32::consts::TAU).sin() * 0.3;
        let mut input = Vec::new();
        for n in 0..48_000 * 3 {
            let v = if (48_000..96_000).contains(&n) {
                0.0
            } else {
                tone(n)
            };
            input.push(v);
            input.push(v);
        }
        let mut out = Vec::new();
        for chunk in input.chunks_exact(hop * 2) {
            w.process(chunk, &mut out);
        }
        // Stale audio would come out as soon as the second burst reaches the
        // model, well before that burst's own delayed onset (the last few
        // hundred frames before it are the analysis window reaching ahead).
        let onset = 96_000 + w.state.latency;
        let quiet: f32 = out[96_000 * 2..(onset - 600) * 2]
            .iter()
            .map(|x| x.abs())
            .fold(0.0, f32::max);
        assert!(quiet < 1e-3, "the first burst came back: peak {quiet}");
    }

    #[test]
    fn zero_frames_is_noop() {
        let (mut e, _) = NoiseSuppressorEffect::new(data(), 48_000, 1024, false);
        let mut buf = vec![0.5f32; 32];
        e.process(&mut buf, 0);
        assert_eq!(buf, vec![0.5; 32]);
    }
}
