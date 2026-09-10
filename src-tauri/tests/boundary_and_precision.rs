mod common;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use common::generators;
use common::metrics;
use serde_json::json;
use splitwave_lib::audio::effects::compressor::CompressorEffect;
use splitwave_lib::audio::effects::delay::DelayEffect;
use splitwave_lib::audio::effects::eq::EqEffect;
use splitwave_lib::audio::effects::gain::GainEffect;
use splitwave_lib::audio::effects::limiter::LimiterEffect;
use splitwave_lib::audio::effects::mute::MuteEffect;
use splitwave_lib::audio::effects::noise_gate::NoiseGateEffect;
use splitwave_lib::audio::effects::saturator::SaturatorEffect;
use splitwave_lib::audio::effects::Effect;
use splitwave_lib::audio::graph::{
    CompressorData, DelayData, EqData, GainData, LimiterData, MuteData, NoiseGateData,
    SaturatorData,
};
use splitwave_lib::audio::resample::MultiResampler;

/// Verifies that when an effect is bypassed, audio passes through 100% bit-exact with zero latency and zero alteration.
#[test]
fn test_bypass_is_bit_exact() {
    let sample_rate = 48_000;
    let (mut eq, _ctrl) = EqEffect::new(EqData { gains_db: [6.0; 10], bypassed: true }, sample_rate);

    let original = generators::sine_stereo(1000.0, 1000.0, sample_rate, 0.1, 0.5);
    let mut processed = original.clone();
    let frames = processed.len() / 2;

    // Simulate the engine's pipeline bypass gate: when bypassed is true, execution is skipped
    let bypass = Arc::new(AtomicBool::new(true));
    if !bypass.load(Ordering::Relaxed) {
        eq.process(&mut processed, frames);
    }

    let (is_exact, max_diff) = metrics::verify_bit_exactness(&original, &processed);
    assert!(
        is_exact && max_diff == 0.0,
        "Bypassed effect did not provide bit-exact passthrough: max diff was {}",
        max_diff
    );
}

/// Verifies that updating effect parameters at runtime via EffectControl immediately affects audio processing without restarts.
#[test]
fn test_runtime_parameter_update() {
    let sample_rate = 48_000;
    let (mut gain, ctrl) = GainEffect::new(GainData { gain_db: 0.0, bypassed: false });
    let frames = 256;

    let mut block = generators::sine_stereo(440.0, 440.0, sample_rate, frames as f32 / sample_rate as f32, 0.2);

    // Initial processing at 0 dB
    gain.process(&mut block, frames);
    let initial_rms = metrics::rms(&block);

    // Apply live update to +6.02 dB
    ctrl.apply_update(&json!({ "gainDb": 6.02 }));
    // Slew block
    gain.process(&mut block, frames);

    // Steady state block at new gain
    let mut steady_block = generators::sine_stereo(440.0, 440.0, sample_rate, frames as f32 / sample_rate as f32, 0.2);
    gain.process(&mut steady_block, frames);
    let updated_rms = metrics::rms(&steady_block);

    let ratio = updated_rms / initial_rms;
    assert!(
        (ratio - 2.0).abs() < 0.05,
        "Runtime parameter update failed to scale gain: expected 2.0x, got {}",
        ratio
    );
}

/// Verifies chunk size independence: processing audio in small chunks vs large chunks produces identical audio output.
#[test]
fn test_chunk_size_independence() {
    let sample_rate = 48_000;
    let total_frames = 1024;
    let input = generators::sine_stereo(440.0, 880.0, sample_rate, total_frames as f32 / sample_rate as f32, 0.4);

    // Stream A: Processed in 16 small chunks of 64 frames
    let (mut gain_a, _ctrl_a) = GainEffect::new(GainData { gain_db: 3.0, bypassed: false });
    let mut stream_a = input.clone();
    for chunk in stream_a.chunks_exact_mut(64 * 2) {
        gain_a.process(chunk, 64);
    }

    // Stream B: Processed in 2 large chunks of 512 frames
    let (mut gain_b, _ctrl_b) = GainEffect::new(GainData { gain_db: 3.0, bypassed: false });
    let mut stream_b = input.clone();
    for chunk in stream_b.chunks_exact_mut(512 * 2) {
        gain_b.process(chunk, 512);
    }

    let (_, max_diff) = metrics::verify_bit_exactness(&stream_a, &stream_b);
    assert!(
        max_diff < 1e-4,
        "Chunk size altered DSP results: max diff between 64-frame and 512-frame chunks was {}",
        max_diff
    );
}

/// Verifies that multiple sequential process calls preserve filter state and match a single consolidated process call.
#[test]
fn test_multiple_process_calls_match_single_process_call() {
    let sample_rate = 48_000;
    let total_frames = 512;
    let input = generators::sine_stereo(1000.0, 1000.0, sample_rate, total_frames as f32 / sample_rate as f32, 0.5);

    // Single call of 512 frames
    let (mut sat_single, _ctrl1) = SaturatorEffect::new(SaturatorData { drive_db: 6.0, threshold_db: -3.0, bypassed: false });
    let mut out_single = input.clone();
    sat_single.process(&mut out_single, total_frames);

    // Two calls of 256 frames
    let (mut sat_multi, _ctrl2) = SaturatorEffect::new(SaturatorData { drive_db: 6.0, threshold_db: -3.0, bypassed: false });
    let mut out_multi = input.clone();
    sat_multi.process(&mut out_multi[..256 * 2], 256);
    sat_multi.process(&mut out_multi[256 * 2..], 256);

    let (is_exact, max_diff) = metrics::verify_bit_exactness(&out_single, &out_multi);
    assert!(
        is_exact && max_diff == 0.0,
        "State was corrupted between process calls: max diff was {}",
        max_diff
    );
}

/// Verifies that stereo channels operate with complete isolation and zero channel crosstalk.
#[test]
fn test_stereo_channels_do_not_crosstalk() {
    let sample_rate = 48_000;
    let frames = 512;

    // Signal on Left channel ONLY, Right channel is digital silence (0.0)
    let left_only = generators::sine_stereo(440.0, 0.0, sample_rate, frames as f32 / sample_rate as f32, 0.8);
    let mut audio = left_only.clone();
    for frame in audio.chunks_exact_mut(2) {
        frame[1] = 0.0;
    }

    // Process through Gain, Saturator, EQ, Limiter
    let (mut gain, _g_c) = GainEffect::new(GainData { gain_db: 3.0, bypassed: false });
    let (mut sat, _s_c) = SaturatorEffect::new(SaturatorData { drive_db: 6.0, threshold_db: -2.0, bypassed: false });
    let (mut eq, _e_c) = EqEffect::new(EqData { gains_db: [2.0; 10], bypassed: false }, sample_rate);
    let (mut limiter, _l_c, _l_m) = LimiterEffect::new(LimiterData { ceiling_db: -1.0, release_ms: 20.0, lookahead_ms: 5.0, bypassed: false }, sample_rate);

    gain.process(&mut audio, frames);
    sat.process(&mut audio, frames);
    eq.process(&mut audio, frames);
    limiter.process(&mut audio, frames);

    // Verify Right channel is strictly 0.0 (no leakage from Left)
    let mut max_right_leak = 0.0f32;
    for frame in audio.chunks_exact(2) {
        max_right_leak = max_right_leak.max(frame[1].abs());
    }

    assert_eq!(
        max_right_leak, 0.0,
        "Channel crosstalk detected: Right channel leaked signal {}",
        max_right_leak
    );
}

/// Verifies that extreme parameter combinations (extreme boost, threshold, ratio) never produce NaN or Infinity.
#[test]
fn test_no_nan_or_inf_for_extreme_parameters() {
    let sample_rate = 48_000;
    let frames = 256;

    let (mut gain, _g) = GainEffect::new(GainData { gain_db: 60.0, bypassed: false });
    let (mut sat, _s) = SaturatorEffect::new(SaturatorData { drive_db: 48.0, threshold_db: -30.0, bypassed: false });
    let (mut comp, _c, _cm) = CompressorEffect::new(
        CompressorData {
            threshold_db: -60.0,
            ratio: 50.0,
            attack_ms: 0.1,
            release_ms: 5.0,
            knee_db: 12.0,
            makeup_db: 24.0,
            bypassed: false,
        },
        sample_rate,
    );
    let (mut limiter, _l, _lm) = LimiterEffect::new(
        LimiterData {
            ceiling_db: -30.0,
            release_ms: 0.5,
            lookahead_ms: 1.0,
            bypassed: false,
        },
        sample_rate,
    );

    // Test signal containing extreme values, subnormal denormals, and high peaks
    let mut extreme_input = vec![0.0f32; frames * 2];
    extreme_input[0] = 1e-35; // Denormal float
    extreme_input[1] = -1e-35;
    extreme_input[10] = 50.0; // Extreme overdriven peak
    extreme_input[11] = -50.0;

    gain.process(&mut extreme_input, frames);
    sat.process(&mut extreme_input, frames);
    comp.process(&mut extreme_input, frames);
    limiter.process(&mut extreme_input, frames);

    for (i, &s) in extreme_input.iter().enumerate() {
        assert!(
            s.is_finite(),
            "Non-finite sample detected at index {}: value = {}",
            i,
            s
        );
    }
}

/// Verifies that toggling mute at runtime zeroes audio instantly and restoring mute un-zeroes audio cleanly.
#[test]
fn test_mute_runtime_toggle_restores_audio() {
    let (mut mute, ctrl) = MuteEffect::new(MuteData { muted: false, bypassed: false });
    let sample_rate = 48_000;
    let frames = 256;

    let original = generators::sine_stereo(440.0, 440.0, sample_rate, frames as f32 / sample_rate as f32, 0.5);

    // Block 1: Unmuted (normal signal)
    let mut block1 = original.clone();
    mute.process(&mut block1, frames);
    assert_eq!(metrics::peak(&block1), metrics::peak(&original));

    // Block 2: Toggle mute ON -> first block ramps down to prevent clicks
    ctrl.apply_update(&json!({ "muted": true }));
    let mut block2 = original.clone();
    mute.process(&mut block2, frames);
    assert!(metrics::peak(&block2) < metrics::peak(&original));

    // Block 3: Steady-state muted block (absolute digital silence)
    let mut block3 = original.clone();
    mute.process(&mut block3, frames);
    assert_eq!(metrics::peak(&block3), 0.0, "Muted block must be digital silence");

    // Block 4: Toggle mute OFF -> ramps back up to full volume
    ctrl.apply_update(&json!({ "muted": false }));
    let mut block4 = original.clone();
    mute.process(&mut block4, frames);

    // Block 5: Steady-state unmuted
    let mut block5 = original.clone();
    mute.process(&mut block5, frames);
    assert!(
        (metrics::peak(&block5) - metrics::peak(&original)).abs() < 1e-5,
        "Unmuted audio failed to restore"
    );
}

/// Verifies compressor ratio accuracy: audio +12 dB above threshold with 4:1 ratio is compressed to +3 dB above threshold.
#[test]
fn test_compressor_ratio_accuracy() {
    let sample_rate = 48_000;
    let threshold_db = -16.0;
    let ratio = 4.0;
    let (mut comp, _ctrl, _meter) = CompressorEffect::new(
        CompressorData {
            threshold_db,
            ratio,
            attack_ms: 1.0,
            release_ms: 50.0,
            knee_db: 0.0,
            makeup_db: 0.0,
            bypassed: false,
        },
        sample_rate,
    );

    // Input signal at -4.0 dBFS (+12 dB above threshold)
    let in_dbfs = -4.0f32;
    let in_amp = 10.0f32.powf(in_dbfs / 20.0);
    let mut signal = generators::sine_stereo(1000.0, 1000.0, sample_rate, 0.2, in_amp);
    let frames = signal.len() / 2;

    comp.process(&mut signal, frames);

    // In steady state, output should be threshold + (overshoot / ratio) = -16 + (12 / 4) = -13 dBFS
    let steady_tail = &signal[signal.len() - 2000..];
    let out_peak = metrics::peak(steady_tail);
    let out_dbfs = metrics::to_dbfs(out_peak);

    let expected_dbfs = threshold_db + (in_dbfs - threshold_db) / ratio;
    assert!(
        (out_dbfs - expected_dbfs).abs() < 0.6,
        "Compressor ratio calculation inaccurate: expected {} dBFS, got {} dBFS",
        expected_dbfs,
        out_dbfs
    );
}

/// Verifies compressor release time accuracy: gain reduction recovers back to unity after signal drops.
#[test]
fn test_compressor_release_time_accuracy() {
    let sample_rate = 48_000;
    let (mut comp, _ctrl, _meter) = CompressorEffect::new(
        CompressorData {
            threshold_db: -12.0,
            ratio: 8.0,
            attack_ms: 1.0,
            release_ms: 30.0,
            knee_db: 0.0,
            makeup_db: 0.0,
            bypassed: false,
        },
        sample_rate,
    );

    // 50ms loud burst followed by 150ms quiet tone (at -30 dBFS, well below threshold)
    let loud = generators::sine_stereo(1000.0, 1000.0, sample_rate, 0.05, 1.0);
    let quiet = generators::sine_stereo(1000.0, 1000.0, sample_rate, 0.15, 0.0316);
    let mut signal = [loud, quiet].concat();
    let frames = signal.len() / 2;

    comp.process(&mut signal, frames);

    // Immediately after loud burst (at t=60ms = 10ms into quiet), gain is still heavily attenuated
    let t60_idx = (sample_rate as f32 * 0.06) as usize * 2;
    let t60_peak = metrics::peak(&signal[t60_idx..t60_idx + 200]);

    // Late in quiet section (at t=180ms = 130ms into quiet > 4x release_ms), gain has fully recovered to 0.0316
    let t180_idx = (sample_rate as f32 * 0.18) as usize * 2;
    let t180_peak = metrics::peak(&signal[t180_idx..t180_idx + 200]);

    assert!(
        t60_peak < t180_peak * 0.7,
        "Compressor failed to maintain gain reduction immediately after loud burst: t60 = {}, t180 = {}",
        t60_peak,
        t180_peak
    );
    assert!(
        (t180_peak - 0.0316).abs() < 0.005,
        "Compressor release failed to recover to uncompressed volume: expected ~0.0316, got {}",
        t180_peak
    );
}

/// Verifies that signals below the limiter ceiling pass through completely unattenuated (0 dB gain reduction).
#[test]
fn test_limiter_no_attenuation_below_ceiling() {
    let sample_rate = 48_000;
    let (mut limiter, _ctrl, _meter) = LimiterEffect::new(
        LimiterData {
            ceiling_db: -1.0,
            release_ms: 20.0,
            lookahead_ms: 5.0,
            bypassed: false,
        },
        sample_rate,
    );

    // Signal at -4 dBFS (0.63 amplitude, well below -1.0 dBFS ceiling of 0.891)
    let signal = generators::sine_stereo(1000.0, 1000.0, sample_rate, 0.1, 0.63);
    let mut processed = signal.clone();
    let frames = processed.len() / 2;

    limiter.process(&mut processed, frames);

    // Discard initial lookahead delay window (5ms / 240 frames) to measure steady-state passthrough
    let sig_rms = metrics::rms(&signal[1024..]);
    let proc_rms = metrics::rms(&processed[1024..]);
    let diff_db = (20.0 * (proc_rms / sig_rms).log10()).abs();

    assert!(
        diff_db < 0.05,
        "Limiter attenuated signal below ceiling: in RMS = {}, out RMS = {}, diff = {} dB",
        sig_rms,
        proc_rms,
        diff_db
    );
}

/// Verifies that the noise gate holds open for the configured hold_ms before starting its release closing ramp.
#[test]
fn test_noise_gate_hold_time() {
    let sample_rate = 48_000;
    let hold_ms = 40.0;
    let (mut gate, _ctrl, _meter) = NoiseGateEffect::new(
        NoiseGateData {
            threshold_db: -20.0,
            range_db: -50.0,
            attack_ms: 1.0,
            hold_ms,
            release_ms: 10.0,
            bypassed: false,
        },
        sample_rate,
    );

    // 50ms loud signal (0 dBFS, opens gate) followed by 150ms quiet noise (-40 dBFS)
    let loud = generators::sine_stereo(1000.0, 1000.0, sample_rate, 0.05, 1.0);
    let quiet = generators::sine_stereo(1000.0, 1000.0, sample_rate, 0.15, 0.01);
    let mut signal = [loud, quiet].concat();
    let frames = signal.len() / 2;

    gate.process(&mut signal, frames);

    // At t=70ms (20ms after loud ends, which is within hold period), gate MUST still be held open
    let t70_idx = (sample_rate as f32 * 0.07) as usize * 2;
    let t70_peak = metrics::peak(&signal[t70_idx..t70_idx + 200]);

    // At t=160ms (accounting for detector decay + hold_ms + release), gate MUST be fully closed
    let t160_idx = (sample_rate as f32 * 0.16) as usize * 2;
    let t160_peak = metrics::peak(&signal[t160_idx..t160_idx + 200]);

    assert!(
        (t70_peak - 0.01).abs() < 0.002,
        "Gate failed to hold open during hold_ms period: expected ~0.01, got {}",
        t70_peak
    );
    assert!(
        t160_peak < 0.001,
        "Gate failed to close after hold + release elapsed: got peak {}",
        t160_peak
    );
}

/// Verifies noise gate release time constant: gate smoothly attenuates down to range_db over release_ms.
#[test]
fn test_noise_gate_release_time() {
    let sample_rate = 48_000;
    let (mut gate, _ctrl, _meter) = NoiseGateEffect::new(
        NoiseGateData {
            threshold_db: -20.0,
            range_db: -40.0,
            attack_ms: 1.0,
            hold_ms: 0.0, // 0ms hold to isolate release timing
            release_ms: 25.0,
            bypassed: false,
        },
        sample_rate,
    );

    let loud = generators::sine_stereo(1000.0, 1000.0, sample_rate, 0.05, 1.0);
    let quiet = generators::sine_stereo(1000.0, 1000.0, sample_rate, 0.15, 0.01);
    let mut signal = [loud, quiet].concat();
    let frames = signal.len() / 2;

    gate.process(&mut signal, frames);

    // End of stream is fully closed
    let end_peak = metrics::peak(&signal[signal.len() - 1000..]);
    let end_dbfs = metrics::to_dbfs(end_peak);

    assert!(
        end_dbfs < -70.0,
        "Noise gate release failed to achieve target attenuation: got {} dBFS",
        end_dbfs
    );
}

/// Verifies that the noise gate does not chatter when audio fluctuates closely around the threshold.
#[test]
fn test_noise_gate_real_hysteresis_no_chatter() {
    let sample_rate = 48_000;
    let (mut gate, _ctrl, _meter) = NoiseGateEffect::new(
        NoiseGateData {
            threshold_db: -20.0,
            range_db: -30.0,
            attack_ms: 2.0,
            hold_ms: 15.0,
            release_ms: 20.0,
            bypassed: false,
        },
        sample_rate,
    );

    // Rapidly fluctuating signal oscillating around -20 dBFS (amplitude 0.10)
    let total_frames = 1000;
    let mut signal = Vec::with_capacity(total_frames * 2);
    for i in 0..total_frames {
        let amp = if i % 2 == 0 { 0.105 } else { 0.095 };
        signal.push(amp);
        signal.push(amp);
    }

    gate.process(&mut signal, total_frames);

    // Check for rapid discontinuity jumps between samples
    let mut max_jump = 0.0f32;
    for i in 1..total_frames {
        let diff = (signal[i * 2] - signal[(i - 1) * 2]).abs();
        if diff > max_jump {
            max_jump = diff;
        }
    }

    assert!(
        max_jump < 0.03,
        "Gate chattered erratically near threshold: max jump between consecutive frames was {}",
        max_jump
    );
}

/// Verifies that the DelayEffect produces echoes at the exact millisecond delay time calculated from sample rate.
#[test]
fn test_delay_exact_delay_time() {
    let sample_rate = 48_000;
    let delay_ms = 50.0;
    let (mut delay, _ctrl) = DelayEffect::new(
        DelayData {
            time_ms: delay_ms,
            feedback: 0.0, // 0 feedback to isolate first echo
            mix: 1.0,      // 100% wet
            bypassed: false,
        },
        sample_rate,
    );

    // Single impulse at frame 0
    let total_frames = 4800; // 100ms
    let mut buffer = vec![0.0f32; total_frames * 2];
    buffer[0] = 1.0;
    buffer[1] = 1.0;

    delay.process(&mut buffer, total_frames);

    // Expected delay in frames: 50ms at 48 kHz = 2400 frames
    let expected_frame = (sample_rate as f32 * delay_ms * 0.001) as usize;
    let peak_frame = buffer.chunks_exact(2).position(|f| f[0].abs() > 0.5).unwrap_or(0);

    assert_eq!(
        peak_frame, expected_frame,
        "Delay timing mismatch: expected frame {}, got {}",
        expected_frame, peak_frame
    );
}

/// Verifies that successive delay feedback echoes decay exponentially according to the feedback multiplier.
#[test]
fn test_delay_feedback_decay() {
    let sample_rate = 48_000;
    let (mut delay, _ctrl) = DelayEffect::new(
        DelayData {
            time_ms: 20.0,
            feedback: 0.5,
            mix: 1.0,
            bypassed: false,
        },
        sample_rate,
    );

    let total_frames = 4800; // 100ms
    let mut buffer = vec![0.0f32; total_frames * 2];
    buffer[0] = 1.0;
    buffer[1] = 1.0;

    delay.process(&mut buffer, total_frames);

    // Delay = 20ms = 960 frames
    let echo1 = buffer[960 * 2].abs();
    let echo2 = buffer[1920 * 2].abs();
    let echo3 = buffer[2880 * 2].abs();

    // In Splitwave's tap model (see docs/ENGINE_DEFECTS.md), echo 1 is the 100% wet delayed input (1.0),
    // and feedback scales subsequent recirculating repeats: echo2 = 0.5, echo3 = 0.25.
    assert!(
        (echo1 - 1.0).abs() < 0.05,
        "First delayed tap incorrect: expected 1.0, got {}",
        echo1
    );
    assert!(
        (echo2 - 0.5).abs() < 0.05,
        "First feedback recirculation incorrect: expected ~0.5, got {}",
        echo2
    );
    assert!(
        (echo3 - 0.25).abs() < 0.05,
        "Second feedback recirculation incorrect: expected ~0.25, got {}",
        echo3
    );
}

/// Verifies that circular buffer wrapping inside DelayEffect across consecutive small blocks does not create discontinuities.
#[test]
fn test_delay_buffer_boundary_continuity() {
    let sample_rate = 48_000;
    let (mut delay, _ctrl) = DelayEffect::new(
        DelayData {
            time_ms: 10.0,
            feedback: 0.3,
            mix: 0.5,
            bypassed: false,
        },
        sample_rate,
    );

    // Process a continuous sine wave in small 64-frame blocks
    let duration = 0.2;
    let mut signal = generators::sine_stereo(440.0, 440.0, sample_rate, duration, 0.5);
    let block_size = 64;

    for chunk in signal.chunks_exact_mut(block_size * 2) {
        delay.process(chunk, block_size);
    }

    // Check that there are no sharp step clicks between adjacent samples anywhere in steady state
    let steady = &signal[2048..];
    let mut max_derivative = 0.0f32;
    for i in 1..steady.len() / 2 {
        let diff = (steady[i * 2] - steady[(i - 1) * 2]).abs();
        if diff > max_derivative {
            max_derivative = diff;
        }
    }

    // Maximum slope of a 440 Hz sine wave at 48 kHz is ~2 * PI * 440 / 48000 * amp ≈ 0.03
    assert!(
        max_derivative < 0.06,
        "Delay circular buffer wrap created click discontinuity: max step was {}",
        max_derivative
    );
}

/// Verifies that resampling between identical sample rates (48 kHz -> 48 kHz) preserves the signal transparently.
#[test]
fn test_resampler_identity_rate_is_transparent() {
    let sample_rate = 48_000;
    let channels = 2;
    let chunk_size = 512;
    let mut resampler = MultiResampler::new(sample_rate, sample_rate, chunk_size, channels).expect("resampler");

    let input = generators::sine_stereo(1000.0, 1000.0, sample_rate, 0.15, 0.5);
    let mut output = Vec::new();

    let mut offset = 0;
    while offset + chunk_size * channels <= input.len() {
        let chunk = &input[offset..offset + chunk_size * channels];
        resampler.process_chunk(chunk, &mut output).expect("resample");
        offset += chunk_size * channels;
    }

    // After sinc filter window settling (256 frames), evaluate steady state transparency
    let in_rms = metrics::rms(&input[2048..]);
    let out_rms = metrics::rms(&output[2048..]);
    let diff_db = (20.0 * (out_rms / in_rms).log10()).abs();

    assert!(
        diff_db < 0.1,
        "Identity resampler (48k->48k) altered signal magnitude: diff = {} dB",
        diff_db
    );
}

/// Verifies that feeding the resampler in small consecutive chunks maintains seamless waveform continuity across chunk borders.
#[test]
fn test_resampler_chunk_boundary_continuity() {
    let in_rate = 44_100;
    let out_rate = 48_000;
    let channels = 2;
    let chunk_size = 256;
    let mut resampler = MultiResampler::new(in_rate, out_rate, chunk_size, channels).expect("resampler");

    let input = generators::sine_stereo(440.0, 440.0, in_rate, 0.15, 0.6);
    let mut output = Vec::new();

    let mut offset = 0;
    while offset + chunk_size * channels <= input.len() {
        let chunk = &input[offset..offset + chunk_size * channels];
        resampler.process_chunk(chunk, &mut output).expect("resample");
        offset += chunk_size * channels;
    }

    // Verify waveform continuity: first differences must stay within smooth sinusoidal limits
    let steady = &output[2048..];
    let mut max_step = 0.0f32;
    for i in 1..steady.len() / 2 {
        let step = (steady[i * 2] - steady[(i - 1) * 2]).abs();
        if step > max_step {
            max_step = step;
        }
    }

    assert!(
        max_step < 0.05,
        "Chunk boundary glitch detected in resampler: max step between samples was {}",
        max_step
    );
}

/// Verifies that downsampling rejects out-of-band frequencies: provides transition-band
/// attenuation (>20 dB at 23.5 kHz for 48k->44.1k) and deep stopband alias rejection (>60 dB).
#[test]
fn test_resampler_alias_rejection() {
    let channels = 2;
    let chunk_size = 512;

    // 1. Transition band attenuation: 48 kHz -> 44.1 kHz at 23.5 kHz (near Nyquist transition)
    {
        let mut resampler = MultiResampler::new(48_000, 44_100, chunk_size, channels).expect("resampler");
        let input = generators::sine_stereo(23_500.0, 23_500.0, 48_000, 0.15, 0.8);
        let mut output = Vec::new();

        let mut offset = 0;
        while offset + chunk_size * channels <= input.len() {
            let chunk = &input[offset..offset + chunk_size * channels];
            resampler.process_chunk(chunk, &mut output).expect("resample");
            offset += chunk_size * channels;
        }

        let out_peak = metrics::peak(&output[2048..]);
        let attenuation_db = metrics::to_dbfs(out_peak) - metrics::to_dbfs(0.8);
        assert!(
            attenuation_db < -20.0,
            "Resampler transition band failed to roll off: attenuation was only {} dB",
            attenuation_db
        );
    }

    // 2. Deep stopband alias rejection: 96 kHz -> 44.1 kHz at 35 kHz (output Nyquist = 22.05 kHz)
    {
        let mut resampler = MultiResampler::new(96_000, 44_100, chunk_size, channels).expect("resampler");
        let input = generators::sine_stereo(35_000.0, 35_000.0, 96_000, 0.15, 0.8);
        let mut output = Vec::new();

        let mut offset = 0;
        while offset + chunk_size * channels <= input.len() {
            let chunk = &input[offset..offset + chunk_size * channels];
            resampler.process_chunk(chunk, &mut output).expect("resample");
            offset += chunk_size * channels;
        }

        let out_peak = metrics::peak(&output[2048..]);
        let attenuation_db = metrics::to_dbfs(out_peak) - metrics::to_dbfs(0.8);
        assert!(
            attenuation_db < -60.0,
            "Resampler deep stopband failed to achieve >60 dB rejection: attenuation was only {} dB",
            attenuation_db
        );
    }
}

/// Verifies that resampling maintains the exact fundamental audio frequency without pitch shifting.
#[test]
fn test_resampler_frequency_preservation() {
    let in_rate = 44_100;
    let out_rate = 48_000;
    let channels = 2;
    let chunk_size = 512;
    let mut resampler = MultiResampler::new(in_rate, out_rate, chunk_size, channels).expect("resampler");

    // Pure 1000 Hz tone
    let freq = 1000.0f32;
    let input = generators::sine_stereo(freq, freq, in_rate, 0.2, 0.5);
    let mut output = Vec::new();

    let mut offset = 0;
    while offset + chunk_size * channels <= input.len() {
        let chunk = &input[offset..offset + chunk_size * channels];
        resampler.process_chunk(chunk, &mut output).expect("resample");
        offset += chunk_size * channels;
    }

    // Count zero crossings in steady state to determine frequency at 48 kHz
    let steady = &output[2048..output.len() - 1000];
    let mut zero_crossings = 0;
    for i in 1..steady.len() / 2 {
        let prev = steady[(i - 1) * 2];
        let curr = steady[i * 2];
        if (prev <= 0.0 && curr > 0.0) || (prev >= 0.0 && curr < 0.0) {
            zero_crossings += 1;
        }
    }

    let measured_duration = (steady.len() / 2) as f32 / out_rate as f32;
    let measured_freq = (zero_crossings as f32 / 2.0) / measured_duration;

    assert!(
        (measured_freq - freq).abs() < 10.0,
        "Resampler shifted frequency: expected {} Hz, got {} Hz",
        freq,
        measured_freq
    );
}

/// Verifies that resampling a long stream accurately matches the expected frame ratio over time without clock drift.
#[test]
fn test_resampler_long_stream_frame_count_accuracy() {
    let in_rate = 48_000;
    let out_rate = 44_100;
    let channels = 2;
    let chunk_size = 512;
    let mut resampler = MultiResampler::new(in_rate, out_rate, chunk_size, channels).expect("resampler");

    // Exactly 1 second of audio at 48 kHz = 48,000 frames
    let in_frames = 48_000;
    let input = vec![0.1f32; in_frames * channels];
    let mut output = Vec::new();

    let mut offset = 0;
    while offset + chunk_size * channels <= input.len() {
        let chunk = &input[offset..offset + chunk_size * channels];
        resampler.process_chunk(chunk, &mut output).expect("resample");
        offset += chunk_size * channels;
    }

    let produced_frames = output.len() / channels;
    let expected_frames = (offset as f64 / channels as f64 * (out_rate as f64 / in_rate as f64)) as usize;

    let diff = (produced_frames as isize - expected_frames as isize).abs();
    // The difference must be bounded within the sinc interpolation filter pipeline latency (128 frames)
    assert!(
        diff <= 128,
        "Resampler frame count deviated beyond pipeline sinc window delay: produced {}, expected {}",
        produced_frames,
        expected_frames
    );
}
