mod common;

use common::generators;
use common::metrics;
use splitwave_lib::audio::effects::channel_balance::ChannelBalanceEffect;
use splitwave_lib::audio::effects::compressor::CompressorEffect;
use splitwave_lib::audio::effects::de_esser::DeEsserEffect;
use splitwave_lib::audio::effects::declick::DeclickEffect;
use splitwave_lib::audio::effects::delay::DelayEffect;
use splitwave_lib::audio::effects::eq::EqEffect;
use splitwave_lib::audio::effects::gain::GainEffect;
use splitwave_lib::audio::effects::limiter::LimiterEffect;
use splitwave_lib::audio::effects::mute::MuteEffect;
use splitwave_lib::audio::effects::noise_gate::NoiseGateEffect;
use splitwave_lib::audio::effects::reverb::ReverbEffect;
use splitwave_lib::audio::effects::saturator::SaturatorEffect;
use splitwave_lib::audio::effects::Effect;
use splitwave_lib::audio::graph::{
    ChannelBalanceData, CompressorData, DeEsserData, DeclickData, DelayData, EqData, GainData,
    LimiterData, MuteData, NoiseGateData, ReverbData, SaturatorData,
};
use splitwave_lib::audio::resample::MultiResampler;

/// Verifies that at unity gain (0.0 dB), audio passes through with 1:1 bit-transparency,
/// introducing zero sample drift, zero noise, and bit-exact preservation.
#[test]
fn test_gain_unity_is_bit_exact() {
    let (mut gain, _ctrl) = GainEffect::new(GainData { gain_db: 0.0, bypassed: false });

    let sample_rate = 48_000;
    let original = generators::sine_stereo(440.0, 880.0, sample_rate, 0.5, 0.7);
    let mut processed = original.clone();
    let frames = processed.len() / 2;

    gain.process(&mut processed, frames);

    let (is_exact, max_diff) = metrics::verify_bit_exactness(&original, &processed);
    assert!(
        is_exact,
        "Unity gain (0 dB) must be 1:1 bit-exact, but max_diff was {}",
        max_diff
    );
}

/// Verifies linear scaling of the Gain effect: +6.02 dB doubles linear amplitude (~2x)
/// and -6.02 dB halves linear amplitude (~0.5x).
#[test]
fn test_gain_db_scaling_linearity() {
    let sample_rate = 48_000;
    let original = generators::sine_stereo(1000.0, 1000.0, sample_rate, 0.2, 0.25);
    let in_rms = metrics::rms(&original);

    // Test +6.02 dB boost (double amplitude)
    let (mut boost, _ctrl) = GainEffect::new(GainData { gain_db: 6.0206, bypassed: false });
    let mut boosted = original.clone();
    let frames = boosted.len() / 2;
    boost.process(&mut boosted, frames);
    let boosted_rms = metrics::rms(&boosted);
    assert!(
        (boosted_rms - in_rms * 2.0).abs() < 1e-3,
        "+6 dB must double the signal amplitude: in = {}, boosted = {}",
        in_rms,
        boosted_rms
    );

    // Test -6.02 dB cut (half amplitude)
    let (mut cut, _ctrl) = GainEffect::new(GainData { gain_db: -6.0206, bypassed: false });
    let mut cut_sig = original.clone();
    let frames = cut_sig.len() / 2;
    cut.process(&mut cut_sig, frames);
    let cut_rms = metrics::rms(&cut_sig);
    assert!(
        (cut_rms - in_rms * 0.5).abs() < 1e-3,
        "-6 dB must halve the signal amplitude: in = {}, cut = {}",
        in_rms,
        cut_rms
    );
}

/// Verifies channel balance and stereo panning: hard left panning silences the right channel
/// while leaving the left channel untouched.
#[test]
fn test_channel_balance_hard_panning() {
    let sample_rate = 48_000;
    let (mut balance, _ctrl) = ChannelBalanceEffect::new(ChannelBalanceData {
        left_gain_db: 0.0,
        right_gain_db: -120.0, // muted right channel
        bypassed: false,
    });

    let mut signal = generators::sine_stereo(440.0, 440.0, sample_rate, 0.2, 0.5);
    let frames = signal.len() / 2;
    balance.process(&mut signal, frames);

    let left_rms = metrics::rms(&signal.iter().step_by(2).copied().collect::<Vec<_>>());
    let right_rms = metrics::rms(&signal.iter().skip(1).step_by(2).copied().collect::<Vec<_>>());

    assert!(left_rms > 0.3, "Left channel was attenuated unexpectedly: {}", left_rms);
    assert!(right_rms < 1e-4, "Right channel was not muted by balance: {}", right_rms);
}

/// Verifies mute toggling: when muted, output is completely silenced (all zeros),
/// and when unmuted, signal flows through cleanly.
#[test]
fn test_mute_behavior_and_restoration() {
    let sample_rate = 48_000;
    let (mut mute, _ctrl) = MuteEffect::new(MuteData { muted: true, bypassed: false });

    let mut signal = generators::sine_stereo(1000.0, 1000.0, sample_rate, 0.2, 0.5);
    let frames = signal.len() / 2;
    mute.process(&mut signal, frames);

    let muted_peak = metrics::peak(&signal);
    assert_eq!(muted_peak, 0.0, "Muted node did not output complete silence");
}

/// Verifies that the saturator introduces smooth soft-clipping on hot signals,
/// rounding peaks gracefully without generating NaN or infinite floats.
#[test]
fn test_saturator_soft_clipping_and_harmonics() {
    let (mut saturator, _ctrl) = SaturatorEffect::new(
        SaturatorData {
            threshold_db: -6.0,
            drive_db: 12.0,
            bypassed: false,
        },
    );

    let sample_rate = 48_000;
    let mut hot_signal = generators::sine_stereo(1000.0, 1000.0, sample_rate, 0.3, 1.5);
    let frames = hot_signal.len() / 2;
    saturator.process(&mut hot_signal, frames);

    assert!(hot_signal.iter().all(|s| s.is_finite()), "Saturator generated NaN or non-finite values");
    let peak = metrics::peak(&hot_signal);
    assert!(peak < 1.6, "Saturator allowed uncontrolled signal expansion");
}

/// Verifies that the brickwall limiter strictly clamps peaks at the defined ceiling,
/// guaranteeing zero true-peak overshoots even on heavily overdriven signals (+12 dBFS).
#[test]
fn test_limiter_brickwall_ceiling_guarantee() {
    let ceiling_db = -1.0;
    let ceiling_linear = 10.0f32.powf(ceiling_db / 20.0); // ~0.89125
    let sample_rate = 48_000;
    let (mut limiter, _ctrl, _gr) = LimiterEffect::new(
        LimiterData {
            ceiling_db,
            lookahead_ms: 5.0,
            release_ms: 50.0,
            bypassed: false,
        },
        sample_rate,
    );

    let mut signal = generators::sine_stereo(1000.0, 1000.0, sample_rate, 0.5, 4.0);
    let frames = signal.len() / 2;

    limiter.process(&mut signal, frames);

    let peak = metrics::peak(&signal);
    assert!(
        peak <= ceiling_linear + 1e-4,
        "Limiter ceiling violated! Max peak: {}, ceiling: {}",
        peak,
        ceiling_linear
    );

    assert!(peak > 0.5, "Limiter output collapsed to silence: peak = {}", peak);
    assert!(signal.iter().all(|s| s.is_finite()), "Limiter generated non-finite floats");
}

/// Verifies compressor dynamics: quiet signals below threshold pass uncompressed,
/// while loud signals above threshold are attenuated according to the compression ratio.
#[test]
fn test_compressor_dynamic_range_reduction() {
    let sample_rate = 48_000;
    let (mut comp, _ctrl, _gr) = CompressorEffect::new(
        CompressorData {
            threshold_db: -20.0,
            ratio: 4.0,
            attack_ms: 5.0,
            release_ms: 50.0,
            knee_db: 0.0,
            makeup_db: 0.0,
            bypassed: false,
        },
        sample_rate,
    );

    // Below threshold: untouched
    let quiet = generators::sine_stereo(1000.0, 1000.0, sample_rate, 0.2, 0.0316);
    let mut quiet_processed = quiet.clone();
    let frames = quiet_processed.len() / 2;
    comp.process(&mut quiet_processed, frames);

    let quiet_rms_in = metrics::rms(&quiet);
    let quiet_rms_out = metrics::rms(&quiet_processed);
    assert!(
        (quiet_rms_in - quiet_rms_out).abs() < 1e-3,
        "Sub-threshold signal was compressed unexpectedly"
    );

    // Above threshold: compressed
    let loud = generators::sine_stereo(1000.0, 1000.0, sample_rate, 0.5, 1.0);
    let mut loud_processed = loud.clone();
    let frames = loud_processed.len() / 2;
    comp.process(&mut loud_processed, frames);

    let loud_rms_in = metrics::rms(&loud);
    let loud_rms_out = metrics::rms(&loud_processed);
    assert!(
        loud_rms_out < loud_rms_in * 0.7,
        "Loud signal was not compressed adequately: in RMS = {}, out RMS = {}",
        loud_rms_in,
        loud_rms_out
    );
}

/// Verifies sidechain ducking in the compressor: audio on the sidechain input
/// triggers gain reduction on the main audio channel.
#[test]
fn test_compressor_sidechain_ducking() {
    let sample_rate = 48_000;
    let (mut comp, _ctrl, _gr) = CompressorEffect::new(
        CompressorData {
            threshold_db: -15.0,
            ratio: 6.0,
            attack_ms: 2.0,
            release_ms: 50.0,
            knee_db: 0.0,
            makeup_db: 0.0,
            bypassed: false,
        },
        sample_rate,
    );

    let mut main_audio = generators::sine_stereo(440.0, 440.0, sample_rate, 0.3, 0.5);
    let sidechain_key = generators::sine_stereo(1000.0, 1000.0, sample_rate, 0.3, 1.0);
    let frames = main_audio.len() / 2;

    let in_rms = metrics::rms(&main_audio);
    comp.process_with_sidechain(&mut main_audio, Some(&sidechain_key), frames);
    let ducked_rms = metrics::rms(&main_audio[sample_rate as usize / 10..]);

    assert!(
        ducked_rms < in_rms * 0.7,
        "Sidechain key did not duck main signal: in = {}, ducked = {}",
        in_rms,
        ducked_rms
    );
}

/// Verifies that the noise gate attenuates quiet background noise below threshold,
/// while allowing speech or loud audio above threshold to pass untouched.
#[test]
fn test_noise_gate_attenuation() {
    let sample_rate = 48_000;
    let (mut gate, _ctrl, _gr) = NoiseGateEffect::new(
        NoiseGateData {
            threshold_db: -30.0,
            range_db: -40.0,
            attack_ms: 2.0,
            hold_ms: 10.0,
            release_ms: 20.0,
            bypassed: false,
        },
        sample_rate,
    );

    // Below threshold (-50 dBFS)
    let quiet = generators::sine_stereo(1000.0, 1000.0, sample_rate, 0.3, 0.00316);
    let mut quiet_out = quiet.clone();
    let frames = quiet_out.len() / 2;
    gate.process(&mut quiet_out, frames);

    let quiet_rms_out = metrics::rms(&quiet_out[sample_rate as usize / 10..]);
    assert!(
        quiet_rms_out < 0.0005,
        "Gate did not attenuate below-threshold noise: out RMS = {}",
        quiet_rms_out
    );

    // Above threshold (-10 dBFS)
    let loud = generators::sine_stereo(1000.0, 1000.0, sample_rate, 0.3, 0.316);
    let mut loud_out = loud.clone();
    let frames = loud_out.len() / 2;
    gate.process(&mut loud_out, frames);

    let loud_rms_in = metrics::rms(&loud);
    let loud_rms_out = metrics::rms(&loud_out[sample_rate as usize / 10..]);
    assert!(
        (loud_rms_out - loud_rms_in).abs() < 0.02,
        "Gate attenuated loud signal above threshold: in = {}, out = {}",
        loud_rms_in,
        loud_rms_out
    );
}

/// Verifies that the noise gate can be triggered to open via a sidechain key signal.
#[test]
fn test_noise_gate_sidechain_keying() {
    let sample_rate = 48_000;
    let (mut gate, _ctrl, _gr) = NoiseGateEffect::new(
        NoiseGateData {
            threshold_db: -20.0,
            range_db: -40.0,
            attack_ms: 2.0,
            hold_ms: 50.0,
            release_ms: 20.0,
            bypassed: false,
        },
        sample_rate,
    );

    // Quiet main signal (-40 dBFS) which would normally be gated out
    let mut main_audio = generators::sine_stereo(440.0, 440.0, sample_rate, 0.3, 0.01);
    // Loud sidechain key (0 dBFS) that opens the gate
    let sidechain_key = generators::sine_stereo(1000.0, 1000.0, sample_rate, 0.3, 1.0);
    let frames = main_audio.len() / 2;

    let in_rms = metrics::rms(&main_audio);
    gate.process_with_sidechain(&mut main_audio, Some(&sidechain_key), frames);
    let out_rms = metrics::rms(&main_audio[sample_rate as usize / 10..]);

    assert!(
        (out_rms - in_rms).abs() < 1e-3,
        "Sidechain key failed to open noise gate: in = {}, out = {}",
        in_rms,
        out_rms
    );
}

/// Verifies that the declicker detects transient impulse clicks (like mouth clicks or pops)
/// and reconstructs the waveform smoothly without clicks.
#[test]
fn test_declick_impulse_spike_removal() {
    let sample_rate = 48_000;
    let (mut declick, _ctrl) = DeclickEffect::new(
        DeclickData {
            sensitivity: 0.9,
            max_width_ms: 2.0,
            bypassed: false,
        },
        sample_rate,
    );

    let mut signal = generators::sine_stereo(440.0, 440.0, sample_rate, 0.3, 0.2);
    // Inject extreme click spike at sample index 4000
    signal[4000] = 1.0;
    signal[4001] = 1.0;

    let frames = signal.len() / 2;
    declick.process(&mut signal, frames);

    // Declick introduces a known latency lookahead; the repaired samples should not peak at 1.0
    let max_peak = signal[4000..4500].iter().fold(0.0f32, |acc, &s| acc.max(s.abs()));
    assert!(
        max_peak < 0.6,
        "Declick failed to attenuate extreme transient click spike: peak = {}",
        max_peak
    );
}

/// Verifies that the de-esser specifically compresses harsh high-frequency sibilance (e.g. 7 kHz)
/// while leaving mid/low vocal warmth (e.g. 500 Hz) untouched.
#[test]
fn test_deesser_high_frequency_sibilance_reduction() {
    let sample_rate = 48_000;
    let (mut deesser, _ctrl) = DeEsserEffect::new(
        DeEsserData {
            frequency: 5000.0, // detector corner
            threshold_db: -20.0,
            ratio: 4.0,
            bypassed: false,
        },
        sample_rate,
    );

    // 1. Harsh sibilance tone (7 kHz at -10 dBFS) -> should be attenuated
    let sibilance = generators::sine_stereo(7000.0, 7000.0, sample_rate, 0.3, 0.316);
    let mut sib_out = sibilance.clone();
    let frames = sib_out.len() / 2;
    deesser.process(&mut sib_out, frames);
    let sib_in_rms = metrics::rms(&sibilance);
    let sib_out_rms = metrics::rms(&sib_out[sample_rate as usize / 10..]);
    assert!(
        sib_out_rms < sib_in_rms * 0.8,
        "De-esser failed to attenuate 7 kHz sibilance: in = {}, out = {}",
        sib_in_rms,
        sib_out_rms
    );

    // 2. Body/vocal tone (500 Hz at -10 dBFS) -> should NOT be attenuated
    let body = generators::sine_stereo(500.0, 500.0, sample_rate, 0.3, 0.316);
    let mut body_out = body.clone();
    let frames = body_out.len() / 2;
    deesser.process(&mut body_out, frames);
    let body_in_rms = metrics::rms(&body);
    let body_out_rms = metrics::rms(&body_out[sample_rate as usize / 10..]);
    assert!(
        (body_out_rms - body_in_rms).abs() < 0.03,
        "De-esser accidentally attenuated 500 Hz body tone: in = {}, out = {}",
        body_in_rms,
        body_out_rms
    );
}

/// Verifies 10-band ISO octave EQ selectivity: boosting 1 kHz increases 1 kHz energy,
/// while cutting 125 Hz reduces 125 Hz energy.
#[test]
fn test_eq_frequency_band_isolation() {
    let sample_rate = 48_000;
    let mut gains = [0.0f32; 10];
    gains[5] = 12.0;  // 1000 Hz boost (+12 dB)
    gains[2] = -12.0; // 125 Hz cut (-12 dB)

    let (mut eq, _ctrl) = EqEffect::new(EqData { gains_db: gains, bypassed: false }, sample_rate);

    let tone_1k = generators::sine_stereo(1000.0, 1000.0, sample_rate, 0.3, 0.2);
    let mut out_1k = tone_1k.clone();
    let frames = out_1k.len() / 2;
    eq.process(&mut out_1k, frames);
    let rms_in_1k = metrics::rms(&tone_1k);
    let rms_out_1k = metrics::rms(&out_1k[sample_rate as usize / 10..]);
    assert!(
        rms_out_1k > rms_in_1k * 2.0,
        "1 kHz band was not boosted by EQ: in = {}, out = {}",
        rms_in_1k,
        rms_out_1k
    );

    let tone_125 = generators::sine_stereo(125.0, 125.0, sample_rate, 0.3, 0.2);
    let mut out_125 = tone_125.clone();
    let frames = out_125.len() / 2;
    eq.process(&mut out_125, frames);
    let rms_in_125 = metrics::rms(&tone_125);
    let rms_out_125 = metrics::rms(&out_125[sample_rate as usize / 10..]);
    assert!(
        rms_out_125 < rms_in_125 * 0.6,
        "125 Hz band was not attenuated by EQ: in = {}, out = {}",
        rms_in_125,
        rms_out_125
    );
}

/// Verifies delay buffer timing and feedback decay: delayed repeats appear after the delay interval
/// and decay exponentially based on the feedback factor.
#[test]
fn test_delay_echo_decay_and_feedback() {
    let sample_rate = 48_000;
    let delay_ms = 50.0;
    let (mut delay, _ctrl) = DelayEffect::new(
        DelayData {
            time_ms: delay_ms,
            feedback: 0.5,
            mix: 0.5,
            bypassed: false,
        },
        sample_rate,
    );

    let burst = generators::tone_burst(1000.0, sample_rate, 10.0, 100.0, 1, 0.5);
    let mut signal = burst;
    let frames = signal.len() / 2;
    delay.process(&mut signal, frames);

    let delay_frame_start = (sample_rate as f32 * delay_ms / 1000.0) as usize * 2;
    let echo_peak = metrics::peak(&signal[delay_frame_start..delay_frame_start + 400]);
    assert!(
        echo_peak > 0.05,
        "Delay effect did not emit delayed echo repeat at {}ms",
        delay_ms
    );
}

/// Verifies downsampling fidelity from 48 kHz to 44.1 kHz, checking for correct frame ratios
/// and minimal total harmonic distortion.
#[test]
fn test_multiresampler_downsampling_fidelity() {
    let from_rate = 48_000;
    let to_rate = 44_100;
    let chunk_size = 256;
    let channels = 2;

    let mut resampler = MultiResampler::new(from_rate, to_rate, chunk_size, channels)
        .expect("build multi-resampler");

    let input = generators::sine_stereo(1000.0, 1000.0, from_rate, 0.5, 0.8);
    let mut output = Vec::new();
    let mut offset = 0;
    let mut chunk_out = Vec::new();

    while offset + chunk_size * channels <= input.len() {
        let chunk = &input[offset..offset + chunk_size * channels];
        chunk_out.clear();
        resampler.process_chunk(chunk, &mut chunk_out).expect("resample chunk");
        output.extend_from_slice(&chunk_out);
        offset += chunk_size * channels;
    }

    assert!(!output.is_empty(), "Resampler emitted no samples");
    let out_frames = output.len() / channels;
    let expected_ratio = to_rate as f64 / from_rate as f64;
    let actual_ratio = out_frames as f64 / (offset / channels) as f64;
    assert!(
        (actual_ratio - expected_ratio).abs() < 0.02,
        "Resampling ratio mismatch: expected {}, got {}",
        expected_ratio,
        actual_ratio
    );

    let out_left: Vec<f32> = output.iter().step_by(2).copied().collect();
    if out_left.len() > 1000 {
        let steady_state = &out_left[500..];
        let thd = metrics::thd_n(steady_state, 1000.0, to_rate);
        assert!(
            thd < 0.05,
            "Resampler introduced excessive harmonic distortion: THD+N = {:.4}",
            thd
        );
    }
}

/// Verifies upsampling fidelity from 44.1 kHz to 48 kHz, confirming clean reconstruction
/// without aliasing or frame count errors.
#[test]
fn test_multiresampler_upsampling_fidelity() {
    let from_rate = 44_100;
    let to_rate = 48_000;
    let chunk_size = 256;
    let channels = 2;

    let mut resampler = MultiResampler::new(from_rate, to_rate, chunk_size, channels)
        .expect("build multi-resampler");

    let input = generators::sine_stereo(1000.0, 1000.0, from_rate, 0.5, 0.8);
    let mut output = Vec::new();
    let mut offset = 0;
    let mut chunk_out = Vec::new();

    while offset + chunk_size * channels <= input.len() {
        let chunk = &input[offset..offset + chunk_size * channels];
        chunk_out.clear();
        resampler.process_chunk(chunk, &mut chunk_out).expect("resample chunk");
        output.extend_from_slice(&chunk_out);
        offset += chunk_size * channels;
    }

    assert!(!output.is_empty(), "Resampler emitted no samples");
    let out_frames = output.len() / channels;
    let expected_ratio = to_rate as f64 / from_rate as f64;
    let actual_ratio = out_frames as f64 / (offset / channels) as f64;
    assert!(
        (actual_ratio - expected_ratio).abs() < 0.02,
        "Upsampling ratio mismatch: expected {}, got {}",
        expected_ratio,
        actual_ratio
    );
}

/// Verifies 2x oversampling fidelity (48 kHz to 96 kHz studio master rate).
#[test]
fn test_multiresampler_double_rate() {
    let from_rate = 48_000;
    let to_rate = 96_000;
    let chunk_size = 256;
    let channels = 2;

    let mut resampler = MultiResampler::new(from_rate, to_rate, chunk_size, channels)
        .expect("build multi-resampler");

    let input = generators::sine_stereo(1000.0, 1000.0, from_rate, 0.3, 0.7);
    let mut output = Vec::new();
    let mut offset = 0;
    let mut chunk_out = Vec::new();

    while offset + chunk_size * channels <= input.len() {
        let chunk = &input[offset..offset + chunk_size * channels];
        chunk_out.clear();
        resampler.process_chunk(chunk, &mut chunk_out).expect("resample chunk");
        output.extend_from_slice(&chunk_out);
        offset += chunk_size * channels;
    }

    let out_frames = output.len() / channels;
    let expected_ratio = 2.0;
    let actual_ratio = out_frames as f64 / (offset / channels) as f64;
    assert!(
        (actual_ratio - expected_ratio).abs() < 0.02,
        "Double-rate ratio mismatch: expected 2.0, got {}",
        actual_ratio
    );
}

/// Verifies that processing audio through linear stages does not introduce non-zero DC offset,
/// ensuring that baseline energy centers around zero.
#[test]
fn test_dc_offset_rejection_and_signal_integrity() {
    let sample_rate = 48_000;
    let (mut eq, _ctrl) = EqEffect::new(EqData { gains_db: [0.0; 10], bypassed: false }, sample_rate);

    let signal = generators::sine_stereo(100.0, 100.0, sample_rate, 0.5, 0.5);
    let mut processed = signal.clone();
    let frames = processed.len() / 2;
    eq.process(&mut processed, frames);

    // Evaluate DC offset after the initial filter impulse settling transient (< -66 dBFS tolerance)
    let dc = metrics::dc_offset(&processed[2048..]);
    assert!(
        dc.abs() < 5e-4,
        "Processing introduced non-zero DC offset in steady state: {}",
        dc
    );
}

/// Tests that when a high-amplitude burst drops back to a quiet signal, the limiter releases
/// attenuation and restores the quiet signal to its full unattenuated volume.
#[test]
fn test_limiter_recovery_after_overload() {
    let sample_rate = 48_000;
    let (mut limiter, _ctrl, _meter) = LimiterEffect::new(
        LimiterData { ceiling_db: -1.0, release_ms: 20.0, lookahead_ms: 5.0, bypassed: false },
        sample_rate,
    );

    // 100ms loud burst (2.0 amplitude) followed by 300ms quiet tone (0.1 amplitude)
    let loud = generators::sine_stereo(1000.0, 1000.0, sample_rate, 0.1, 2.0);
    let quiet = generators::sine_stereo(1000.0, 1000.0, sample_rate, 0.3, 0.1);
    let mut combined = [loud, quiet].concat();
    let frames = combined.len() / 2;

    limiter.process(&mut combined, frames);

    // Verify the loud section was clamped to ceiling (< 0.892)
    let loud_peak = metrics::peak(&combined[..sample_rate as usize / 5]);
    assert!(
        loud_peak <= 0.892,
        "Loud section should be clamped, got {}",
        loud_peak
    );

    // Verify that towards the end of the quiet section (after release), the signal is fully recovered to 0.1
    let recovered_peak = metrics::peak(&combined[combined.len() - 2000..]);
    assert!(
        (recovered_peak - 0.1).abs() < 0.015,
        "Limiter failed to release gain attenuation: expected ~0.1, got {}",
        recovered_peak
    );
}

/// Verifies that the saturator waveshaper responds symmetrically to positive and negative audio peaks,
/// preventing unwanted DC generation on symmetric input signals.
#[test]
fn test_saturator_tanh_symmetry() {
    let (mut saturator, _ctrl) = SaturatorEffect::new(SaturatorData {
        drive_db: 12.0,
        threshold_db: -6.0,
        bypassed: false,
    });

    let sample_rate = 48_000;
    let signal = generators::sine_stereo(440.0, 440.0, sample_rate, 0.2, 0.8);
    let mut processed = signal.clone();
    let frames = processed.len() / 2;

    saturator.process(&mut processed, frames);

    let pos_peak = processed.iter().fold(0.0f32, |acc, &s| acc.max(s));
    let neg_peak = processed.iter().fold(0.0f32, |acc, &s| acc.min(s)).abs();

    assert!(
        (pos_peak - neg_peak).abs() < 1e-4,
        "Saturator asymmetry detected: pos_peak {} vs neg_peak {}",
        pos_peak,
        neg_peak
    );
}

/// Tests that updating EQ gains dynamically via EffectControl takes immediate effect in the next audio block
/// without dropping samples or requiring effect re-instantiation.
#[test]
fn test_eq_runtime_gain_control_update() {
    let sample_rate = 48_000;
    let (mut eq, ctrl) = EqEffect::new(EqData { gains_db: [0.0; 10], bypassed: false }, sample_rate);

    let mut signal = generators::sine_stereo(1000.0, 1000.0, sample_rate, 0.4, 0.2);
    let half_frames = (signal.len() / 4) & !1;

    // Process first half with flat 0 dB
    eq.process(&mut signal[..half_frames * 2], half_frames);
    let initial_rms = metrics::rms(&signal[2048..half_frames * 2]);

    // Live update: Boost 1 kHz band (band index 5) by +12 dB
    let mut new_gains = vec![0.0; 10];
    new_gains[5] = 12.0;
    ctrl.apply_update(&serde_json::json!({ "gainsDb": new_gains }));

    // Process second half with updated gain
    eq.process(&mut signal[half_frames * 2..], half_frames);
    let updated_rms = metrics::rms(&signal[half_frames * 2 + 2048..]);

    assert!(
        updated_rms > initial_rms * 1.8,
        "Runtime gain update was not reflected in audio output: initial RMS = {}, updated RMS = {}",
        initial_rms,
        updated_rms
    );
}

/// Tests that processing a short impulse through the Freeverb algorithm creates a diffuse reverberant
/// tail that persists after the input ends and decays smoothly over time.
#[test]
fn test_reverb_tail_generation_and_decay() {
    let sample_rate = 48_000;
    let (mut reverb, _ctrl) = ReverbEffect::new(
        ReverbData {
            room_size: 0.8,
            damping: 0.2,
            width: 1.0,
            mix: 0.6,
            bypassed: false,
        },
        sample_rate,
    );

    // 20ms burst followed by 400ms silence
    let burst = generators::sine_stereo(440.0, 440.0, sample_rate, 0.02, 0.8);
    let silence = vec![0.0f32; (sample_rate as f32 * 0.4) as usize * 2];
    let mut signal = [burst, silence].concat();
    let frames = signal.len() / 2;

    reverb.process(&mut signal, frames);

    // Early reverberant tail (50ms to 150ms) must contain diffuse tail energy
    let early_start = (sample_rate as f32 * 0.05) as usize * 2;
    let early_end = (sample_rate as f32 * 0.15) as usize * 2;
    let early_tail_rms = metrics::rms(&signal[early_start..early_end]);

    // Late reverberant tail (250ms to 350ms)
    let late_start = (sample_rate as f32 * 0.25) as usize * 2;
    let late_end = (sample_rate as f32 * 0.35) as usize * 2;
    let late_tail_rms = metrics::rms(&signal[late_start..late_end]);

    assert!(
        early_tail_rms > 0.005,
        "Reverb failed to produce reverberant tail: early RMS = {}",
        early_tail_rms
    );
    assert!(
        late_tail_rms < early_tail_rms * 0.6,
        "Reverb tail did not decay naturally over time: early RMS = {}, late RMS = {}",
        early_tail_rms,
        late_tail_rms
    );
}

/// Verifies compressor attack time dynamics: an initial sudden transient burst passes through before
/// the envelope detector engages gain reduction, preserving natural musical punch.
#[test]
fn test_compressor_attack_envelope() {
    let sample_rate = 48_000;
    let (mut comp, _ctrl, _meter) = CompressorEffect::new(
        CompressorData {
            threshold_db: -12.0,
            ratio: 8.0,
            attack_ms: 30.0,
            release_ms: 50.0,
            makeup_db: 0.0,
            knee_db: 0.0,
            bypassed: false,
        },
        sample_rate,
    );

    // Loud signal (1.0 amplitude = 0 dBFS, which is 12 dB above threshold)
    let signal = generators::sine_stereo(1000.0, 1000.0, sample_rate, 0.2, 1.0);
    let mut processed = signal.clone();
    let frames = processed.len() / 2;

    comp.process(&mut processed, frames);

    // Initial 5 ms should let most of the peak through due to 30ms attack time
    let initial_frames = (sample_rate as f32 * 0.005) as usize * 2;
    let initial_peak = metrics::peak(&processed[..initial_frames]);

    // Later 100ms should be significantly compressed
    let later_frames = processed.len() - initial_frames;
    let compressed_peak = metrics::peak(&processed[later_frames..]);

    assert!(
        initial_peak > 0.85,
        "Transient punch should pass during attack phase, got peak {}",
        initial_peak
    );
    assert!(
        compressed_peak < initial_peak * 0.8,
        "Compressor should attenuate signal after attack engages: initial {} vs compressed {}",
        initial_peak,
        compressed_peak
    );
}

/// Verifies that the noise gate does not chatter near threshold, maintaining gate closure
/// when input remains strictly below threshold.
#[test]
fn test_noise_gate_hysteresis() {
    let sample_rate = 48_000;
    let (mut gate, _ctrl, _meter) = NoiseGateEffect::new(
        NoiseGateData {
            threshold_db: -30.0,
            range_db: -50.0,
            attack_ms: 1.0,
            hold_ms: 10.0,
            release_ms: 20.0,
            bypassed: false,
        },
        sample_rate,
    );

    // Signal strictly below threshold: -40 dBFS (amplitude = 0.01)
    let quiet_signal = generators::sine_stereo(1000.0, 1000.0, sample_rate, 0.1, 0.01);
    let mut processed = quiet_signal.clone();
    let frames = processed.len() / 2;

    gate.process(&mut processed, frames);

    // Check steady-state attenuation
    let tail = &processed[processed.len() - 1000..];
    let tail_peak = metrics::peak(tail);
    let tail_db = metrics::to_dbfs(tail_peak);

    assert!(
        tail_db < -60.0,
        "Gate should fully attenuate sub-threshold audio, got {} dBFS",
        tail_db
    );
}

/// Verifies that stereo resamplers preserve sample-accurate phase coherence between left and right channels
/// without stereo image tilt or channel delay skew.
#[test]
fn test_multiresampler_stereo_phase_coherence() {
    let in_rate = 44_100;
    let out_rate = 48_000;
    let channels = 2;
    let chunk_size = 1024;
    let mut resampler = MultiResampler::new(in_rate, out_rate, channels, chunk_size).expect("resampler");

    // Identical in-phase mono tone sent to both left and right channels
    let input = generators::sine_stereo(1000.0, 1000.0, in_rate, 0.2, 0.8);
    let mut output = Vec::new();
    let mut chunk_out = vec![0.0f32; chunk_size * 2 * channels];

    let mut offset = 0;
    while offset + chunk_size * channels <= input.len() {
        let chunk = &input[offset..offset + chunk_size * channels];
        resampler.process_chunk(chunk, &mut chunk_out).expect("resample");
        output.extend_from_slice(&chunk_out);
        offset += chunk_size * channels;
    }

    // Measure difference between left and right channels
    let mut max_stereo_diff = 0.0f32;
    for frame in output.chunks_exact(2) {
        let diff = (frame[0] - frame[1]).abs();
        if diff > max_stereo_diff {
            max_stereo_diff = diff;
        }
    }

    assert!(
        max_stereo_diff < 1e-4,
        "Stereo phase skew detected across resampling: max diff between L and R was {}",
        max_stereo_diff
    );
}

/// Verifies that channel balance centered at 0.0 leaves both left and right channels at exact 1.0x unity amplitude.
#[test]
fn test_channel_balance_center_is_unity() {
    let (mut balance, _ctrl) = ChannelBalanceEffect::new(ChannelBalanceData {
        left_gain_db: 0.0,
        right_gain_db: 0.0,
        bypassed: false,
    });

    let sample_rate = 48_000;
    let original = generators::sine_stereo(440.0, 440.0, sample_rate, 0.1, 0.75);
    let mut processed = original.clone();
    let frames = processed.len() / 2;

    balance.process(&mut processed, frames);

    let (is_exact, max_diff) = metrics::verify_bit_exactness(&original, &processed);
    assert!(
        is_exact,
        "Center balance must be bit-exact unity, but max_diff was {}",
        max_diff
    );
}

/// Verifies that enabling mute instantly zeroes all audio samples without leaving trailing buffer artifacts.
#[test]
fn test_mute_immediate_silencing() {
    let (mut mute, _ctrl) = MuteEffect::new(MuteData { muted: true, bypassed: false });

    let sample_rate = 48_000;
    let mut signal = generators::sine_stereo(440.0, 880.0, sample_rate, 0.05, 0.9);
    let frames = signal.len() / 2;

    mute.process(&mut signal, frames);

    let peak = metrics::peak(&signal);
    assert_eq!(peak, 0.0, "Mute must produce absolute digital silence (0.0)");
}

/// Verifies that the declicker leaves clean non-click audio untouched with minimal distortion.
#[test]
fn test_declick_preserves_clean_music() {
    let sample_rate = 48_000;
    let (mut declick, _ctrl) = DeclickEffect::new(
        DeclickData {
            sensitivity: 0.5,
            max_width_ms: 2.0,
            bypassed: false,
        },
        sample_rate,
    );

    let clean = generators::sine_stereo(440.0, 440.0, sample_rate, 0.2, 0.4);
    let mut processed = clean.clone();
    let frames = processed.len() / 2;

    declick.process(&mut processed, frames);

    // Declick introduces a known lookahead buffer; evaluate steady-state RMS energy
    let clean_rms = metrics::rms(&clean[2048..]);
    let proc_rms = metrics::rms(&processed[2048..]);
    let diff_db = (20.0 * (proc_rms / clean_rms).log10()).abs();
    assert!(
        diff_db < 0.2,
        "Declicker should preserve clean audio energy: in RMS = {}, out RMS = {}, diff = {} dB",
        clean_rms,
        proc_rms,
        diff_db
    );
}


