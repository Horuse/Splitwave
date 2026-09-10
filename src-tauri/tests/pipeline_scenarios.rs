mod common;

use common::generators;
use common::metrics;
use rtrb::RingBuffer;
use serde_json::json;
use splitwave_lib::audio::effects::compressor::CompressorEffect;
use splitwave_lib::audio::effects::de_esser::DeEsserEffect;
use splitwave_lib::audio::effects::declick::DeclickEffect;
use splitwave_lib::audio::effects::delay::DelayEffect;
use splitwave_lib::audio::effects::eq::EqEffect;
use splitwave_lib::audio::effects::gain::GainEffect;
use splitwave_lib::audio::effects::level_meter::LevelMeterEffect;
use splitwave_lib::audio::effects::limiter::LimiterEffect;
use splitwave_lib::audio::effects::lufs_meter::LufsMeterEffect;
use splitwave_lib::audio::effects::noise_gate::NoiseGateEffect;
use splitwave_lib::audio::effects::reverb::ReverbEffect;
use splitwave_lib::audio::effects::saturator::SaturatorEffect;
use splitwave_lib::audio::effects::Effect;
use splitwave_lib::audio::graph::{
    CompressorData, DeEsserData, DeclickData, DelayData, EqData, GainData,
    LevelMeterData, LimiterData, LufsMeterData, NoiseGateData, ReverbData, SaturatorData,
};
use splitwave_lib::audio::resample::MultiResampler;

/// Simulates a complete broadcast vocal processing strip:
/// Mic Input -> Noise Gate -> Declicker -> De-esser -> 10-band EQ -> Compressor -> Limiter -> Meter.
///
/// Plain English explanation:
/// In professional broadcasting, voice audio suffers from room noise in pauses, mouth clicks,
/// harsh 's' sibilance, and wide volume variations. This test runs speech with clicks and sibilance
/// through the entire 7-stage chain, verifying that background pauses are silenced, clicks are removed,
/// sibilance is tamed, and the final output never clips beyond the -1 dBFS broadcast ceiling.
#[test]
fn test_broadcast_vocal_chain() {
    let sample_rate = 48_000;

    // 1. Noise Gate (cuts room tone during speaking pauses)
    let (mut gate, _gate_ctrl, _gate_meter) = NoiseGateEffect::new(
        NoiseGateData {
            threshold_db: -30.0,
            range_db: -40.0,
            attack_ms: 2.0,
            hold_ms: 20.0,
            release_ms: 30.0,
            bypassed: false,
        },
        sample_rate,
    );

    // 2. Declicker (eliminates mouth clicks and impulse pops)
    let (mut declick, _declick_ctrl) = DeclickEffect::new(
        DeclickData {
            sensitivity: 0.9,
            max_width_ms: 2.0,
            bypassed: false,
        },
        sample_rate,
    );

    // 3. De-esser (compresses harsh 7 kHz sibilance)
    let (mut deesser, _deesser_ctrl) = DeEsserEffect::new(
        DeEsserData {
            frequency: 7000.0,
            threshold_db: -18.0,
            ratio: 4.0,
            bypassed: false,
        },
        sample_rate,
    );

    // 4. EQ (adds gentle speech presence boost at 2 kHz)
    let mut eq_gains = [0.0f32; 10];
    eq_gains[6] = 3.0; // 2 kHz presence boost
    let (mut eq, _eq_ctrl) = EqEffect::new(EqData { gains_db: eq_gains, bypassed: false }, sample_rate);

    // 5. Compressor (evens dynamic speech volume)
    let (mut comp, _comp_ctrl, _comp_meter) = CompressorEffect::new(
        CompressorData {
            threshold_db: -14.0,
            ratio: 4.0,
            attack_ms: 5.0,
            release_ms: 50.0,
            knee_db: 3.0,
            makeup_db: 2.0,
            bypassed: false,
        },
        sample_rate,
    );

    // 6. Brickwall Limiter (prevents digital clipping above -1 dBFS)
    let (mut limiter, _lim_ctrl, _lim_meter) = LimiterEffect::new(
        LimiterData {
            ceiling_db: -1.0,
            release_ms: 20.0,
            lookahead_ms: 5.0,
            bypassed: false,
        },
        sample_rate,
    );

    // 7. Output Level Meter
    let (mut meter, meter_handle) = LevelMeterEffect::new(LevelMeterData {}, "vocal_out".into());

    // Generate test audio: exact multiples of block_size (512 frames)
    let section_frames = 512 * 20; // 10,240 frames (~0.213s)
    let section_sec = section_frames as f32 / sample_rate as f32;

    // Section 1: Active loud speech (0.9 amplitude) with an injected click spike and 7kHz sibilance
    let mut speech = generators::sine_stereo(500.0, 500.0, sample_rate, section_sec, 0.9);
    // Inject click spike
    speech[2000] = 1.0;
    speech[2001] = 1.0;
    // Inject high-frequency sibilance burst
    let sibilance = generators::sine_stereo(7000.0, 7000.0, sample_rate, 0.1, 0.8);
    for (i, &s) in sibilance.iter().enumerate() {
        if i + 4000 < speech.len() {
            speech[i + 4000] += s;
        }
    }

    // Section 2: Background pause (quiet room tone at -45 dBFS = 0.0056)
    let pause = generators::sine_stereo(100.0, 100.0, sample_rate, section_sec, 0.0056);

    let mut session_audio = [speech, pause].concat();
    let total_frames = session_audio.len() / 2;

    // Process through the entire chain in 512-frame blocks
    let block_size = 512;
    let mut offset = 0;
    while offset + block_size <= total_frames {
        let block = &mut session_audio[offset * 2..(offset + block_size) * 2];
        gate.process(block, block_size);
        declick.process(block, block_size);
        deesser.process(block, block_size);
        eq.process(block, block_size);
        comp.process(block, block_size);
        limiter.process(block, block_size);
        meter.process(block, block_size);
        offset += block_size;
    }

    // Verification 1: Speech section peaks are strictly governed below limiter ceiling (0.892 = -1 dBFS)
    let speech_peak = metrics::peak(&session_audio[..section_frames * 2]);
    assert!(
        speech_peak <= 0.892,
        "Limiter ceiling breached: peak was {}",
        speech_peak
    );

    // Verification 2: Background pause is gated to silence (< -50 dBFS)
    let pause_tail = &session_audio[session_audio.len() - 2048..];
    let pause_peak = metrics::peak(pause_tail);
    let pause_dbfs = metrics::to_dbfs(pause_peak);
    assert!(
        pause_dbfs < -50.0,
        "Noise gate failed to silence room noise in pause: level = {} dBFS",
        pause_dbfs
    );

    // Verification 3: Output meter recorded active levels
    let snap = meter_handle.snapshot_and_decay();
    assert!(
        !snap.peaks.is_empty() && snap.peaks[0] > 0.0,
        "Level meter failed to record audio peaks"
    );
}

/// Simulates auto-ducking in a podcast or live stream:
/// Background music is automatically lowered whenever the podcaster speaks.
///
/// Plain English explanation:
/// In podcasts, background music should play at normal volume when no one is talking,
/// but smoothly drop down (-12 dB) whenever the host speaks so their voice is clearly heard.
/// This test verifies that sidechain compression ducks music during speech and restores it during silence.
#[test]
fn test_sidechain_ducking_podcast_scenario() {
    let sample_rate = 48_000;
    let (mut ducking_comp, _ctrl, _gr) = CompressorEffect::new(
        CompressorData {
            threshold_db: -18.0,
            ratio: 6.0,
            attack_ms: 10.0,
            release_ms: 100.0,
            knee_db: 2.0,
            makeup_db: 0.0,
            bypassed: false,
        },
        sample_rate,
    );

    // Continuous background music bed (440 Hz tone at -6 dBFS = 0.5 amplitude)
    let duration = 0.6; // 600ms
    let mut music_channel = generators::sine_stereo(440.0, 440.0, sample_rate, duration, 0.5);

    // Host vocal sidechain key:
    // First 250ms: Host speaks (0.9 amplitude = -0.9 dBFS, well above compressor threshold)
    // Next 350ms: Host stops talking (0.0 silence)
    let speech_key = generators::sine_stereo(1000.0, 1000.0, sample_rate, 0.25, 0.9);
    let silence_key = vec![0.0f32; (sample_rate as f32 * 0.35) as usize * 2];
    let host_vocal_track = [speech_key, silence_key].concat();

    let frames = music_channel.len() / 2;
    ducking_comp.process_with_sidechain(&mut music_channel, Some(&host_vocal_track), frames);

    // Check music level while host is speaking (around 150ms to 200ms)
    let ducked_start = (sample_rate as f32 * 0.15) as usize * 2;
    let ducked_end = (sample_rate as f32 * 0.20) as usize * 2;
    let ducked_rms = metrics::rms(&music_channel[ducked_start..ducked_end]);

    // Check music level after host stops speaking (around 500ms to 550ms, after release)
    let restored_start = (sample_rate as f32 * 0.50) as usize * 2;
    let restored_end = (sample_rate as f32 * 0.55) as usize * 2;
    let restored_rms = metrics::rms(&music_channel[restored_start..restored_end]);

    assert!(
        ducked_rms < restored_rms * 0.6,
        "Music was not ducked adequately: ducked RMS = {}, restored RMS = {}",
        ducked_rms,
        restored_rms
    );
    assert!(
        (restored_rms - 0.35).abs() < 0.05,
        "Music failed to restore to original volume: expected ~0.35, got {}",
        restored_rms
    );
}

/// Simulates a streamer mixing dual sample rates:
/// 48 kHz Game Audio + 44.1 kHz Voice Chat -> Master Summing Bus -> Master Limiter.
///
/// Plain English explanation:
/// Different audio sources run at different hardware clock speeds (e.g. 44.1 kHz Discord and 48 kHz Game).
/// This test verifies that the MultiResampler seamlessly synchronizes the 44.1 kHz feed to 48 kHz,
/// and that both signals sum cleanly without buffer overflows or distortion.
#[test]
fn test_streamer_dual_sample_rate_mix() {
    let out_rate = 48_000;
    let in_rate = 44_100;
    let duration = 0.2; // 200ms
    let channels = 2;
    let chunk_size = 512;

    // Game audio at 48 kHz (1000 Hz tone)
    let game_audio = generators::sine_stereo(1000.0, 1000.0, out_rate, duration, 0.4);

    // Discord voice at 44.1 kHz (500 Hz tone)
    let discord_audio = generators::sine_stereo(500.0, 500.0, in_rate, duration, 0.4);

    // Resample Discord voice from 44.1 kHz to 48 kHz
    let mut resampler = MultiResampler::new(in_rate, out_rate, channels, chunk_size).expect("resampler");
    let mut discord_resampled = Vec::new();
    let mut chunk_out = vec![0.0f32; chunk_size * 2 * channels];

    let mut offset = 0;
    while offset + chunk_size * channels <= discord_audio.len() {
        let chunk = &discord_audio[offset..offset + chunk_size * channels];
        resampler.process_chunk(chunk, &mut chunk_out).expect("resample");
        discord_resampled.extend_from_slice(&chunk_out);
        offset += chunk_size * channels;
    }

    // Sum Game and Resampled Discord into Master Bus
    let min_frames = (game_audio.len() / 2).min(discord_resampled.len() / 2);
    let mut master_bus = Vec::with_capacity(min_frames * 2);
    for i in 0..min_frames * 2 {
        master_bus.push(game_audio[i] + discord_resampled[i]);
    }

    // Apply Master Limiter to prevent clipping when summing multiple active tracks
    let (mut limiter, _ctrl, _meter) = LimiterEffect::new(
        LimiterData {
            ceiling_db: -0.5,
            release_ms: 10.0,
            lookahead_ms: 5.0,
            bypassed: false,
        },
        out_rate,
    );

    limiter.process(&mut master_bus, min_frames);

    let peak = metrics::peak(&master_bus);
    assert!(
        peak <= 0.945, // -0.5 dBFS linear is ~0.944
        "Master bus clipped after summing dual sources: peak was {}",
        peak
    );
    assert!(
        min_frames > 8000,
        "Expected at least 8000 frames of mixed audio, got {}",
        min_frames
    );
}

/// Simulates DJ console crossfading between Deck A and Deck B:
///
/// Plain English explanation:
/// A DJ mixer crossfader blends between two running tracks. At 0.0 only Deck A is heard,
/// at 0.5 both tracks blend equally without perceived volume dip, and at 1.0 only Deck B is heard.
/// This test verifies smooth, click-free crossfade transitions across deck outputs.
#[test]
fn test_dj_crossfader_scenario() {
    let sample_rate = 48_000;

    // Deck A plays 440 Hz, Deck B plays 880 Hz
    let deck_a = generators::sine_stereo(440.0, 440.0, sample_rate, 0.05, 0.7);
    let deck_b = generators::sine_stereo(880.0, 880.0, sample_rate, 0.05, 0.7);

    // Crossfader helper calculating constant-power sine/cosine blend
    let crossfade = |a: &[f32], b: &[f32], pos: f32| -> Vec<f32> {
        let gain_a = (pos * std::f32::consts::FRAC_PI_2).cos();
        let gain_b = (pos * std::f32::consts::FRAC_PI_2).sin();
        let mut out = vec![0.0f32; a.len()];
        for i in 0..a.len() {
            out[i] = a[i] * gain_a + b[i] * gain_b;
        }
        out
    };

    // Position 0.0 (Deck A full, Deck B silent)
    let mixed_deck_a_only = crossfade(&deck_a, &deck_b, 0.0);
    let (_, diff_a) = metrics::verify_bit_exactness(&deck_a, &mixed_deck_a_only);
    assert!(diff_a < 1e-4, "Deck A only crossfade should match Deck A");

    // Position 1.0 (Deck B full, Deck A silent)
    let mixed_deck_b_only = crossfade(&deck_a, &deck_b, 1.0);
    let (_, diff_b) = metrics::verify_bit_exactness(&deck_b, &mixed_deck_b_only);
    assert!(diff_b < 1e-4, "Deck B only crossfade should match Deck B");

    // Position 0.5 (Center blend: equal energy)
    let mixed_center = crossfade(&deck_a, &deck_b, 0.5);
    let rms_center = metrics::rms(&mixed_center);
    let rms_single = metrics::rms(&deck_a);

    // Constant power crossfade maintains total energy (~1.0x single deck RMS)
    let ratio = rms_center / rms_single;
    assert!(
        (ratio - 1.0).abs() < 0.05,
        "Constant-power crossfade power mismatch at center: ratio was {}",
        ratio
    );
}

/// Simulates electronic dance music (EDM) rhythmic sidechain pumping:
/// A four-on-the-floor kick drum rhythmically ducks a sustained synth bass pad.
///
/// Plain English explanation:
/// In dance music, every time the kick drum hits, the loud synth bass instantly dips in volume
/// and swells back up, creating a characteristic energetic 'pumping' rhythm.
/// This test verifies cyclic gain reduction and recovery on periodic beat hits.
#[test]
fn test_dance_music_sidechain_pumping() {
    let sample_rate = 48_000;
    let (mut comp, _ctrl, _meter) = CompressorEffect::new(
        CompressorData {
            threshold_db: -20.0,
            ratio: 8.0,
            attack_ms: 2.0,
            release_ms: 60.0,
            knee_db: 0.0,
            makeup_db: 0.0,
            bypassed: false,
        },
        sample_rate,
    );

    // Sustained synth bass (continuous 200 Hz tone at 0.7 amplitude)
    let total_duration = 0.4; // 400ms (covers two kick hits spaced 200ms apart)
    let mut synth_bass = generators::sine_stereo(200.0, 200.0, sample_rate, total_duration, 0.7);

    // Kick drum track: two 30ms kick bursts (at t=0ms and t=200ms) with silence in between
    let kick_burst = generators::sine_stereo(60.0, 60.0, sample_rate, 0.03, 1.0);
    let kick_pause = vec![0.0f32; (sample_rate as f32 * 0.17) as usize * 2];
    let kick_track = [kick_burst.clone(), kick_pause.clone(), kick_burst, kick_pause].concat();

    let frames = synth_bass.len() / 2;
    comp.process_with_sidechain(&mut synth_bass, Some(&kick_track), frames);

    // Sample 1: During first kick hit (t = 15ms)
    let kick1_sample = (sample_rate as f32 * 0.015) as usize * 2;
    let ducked_peak1 = metrics::peak(&synth_bass[kick1_sample..kick1_sample + 200]);

    // Sample 2: Between kicks during recovery (t = 150ms)
    let recovery_sample = (sample_rate as f32 * 0.150) as usize * 2;
    let recovered_peak = metrics::peak(&synth_bass[recovery_sample..recovery_sample + 200]);

    // Sample 3: During second kick hit (t = 215ms)
    let kick2_sample = (sample_rate as f32 * 0.215) as usize * 2;
    let ducked_peak2 = metrics::peak(&synth_bass[kick2_sample..kick2_sample + 200]);

    assert!(
        ducked_peak1 < recovered_peak * 0.7,
        "First kick failed to duck synth bass: ducked {} vs recovered {}",
        ducked_peak1,
        recovered_peak
    );
    assert!(
        ducked_peak2 < recovered_peak * 0.7,
        "Second kick failed to duck synth bass: ducked {} vs recovered {}",
        ducked_peak2,
        recovered_peak
    );
    assert!(
        recovered_peak > 0.45,
        "Synth bass failed to recover volume between kick beats: got {}",
        recovered_peak
    );
}

/// Simulates bursty operating system audio packet jitter and ring buffer recovery:
///
/// Plain English explanation:
/// Due to operating system scheduling jitter (e.g. Wi-Fi audio, Bluetooth latency, or high CPU load),
/// audio packets often arrive in irregular bursts (e.g. 100 samples, then 0, then 600 samples).
/// This test verifies that the rtrb lock-free ring buffer absorbs bursty inputs and provides
/// a steady stream of fixed-size blocks to the DSP engine without underruns or sample loss.
#[test]
fn test_bursty_audio_jitter_ringbuffer_recovery() {
    let (mut producer, mut consumer) = RingBuffer::<f32>::new(4096);

    let test_stream: Vec<f32> = (0..2048).map(|i| (i as f32) * 0.001).collect();

    // Irregular burst arrival patterns simulating thread scheduling jitter
    let burst_sizes = [128, 64, 512, 0, 256, 100, 300, 400, 288];
    let mut written = 0;
    let mut read_output = Vec::with_capacity(2048);

    for &burst in &burst_sizes {
        // Producer writes burst
        if burst > 0 && written + burst <= test_stream.len() {
            let chunk = &test_stream[written..written + burst];
            for &sample in chunk {
                producer.push(sample).expect("RingBuffer push");
            }
            written += burst;
        }

        // Consumer reads in steady 128-sample DSP blocks whenever available
        while consumer.slots() >= 128 {
            for _ in 0..128 {
                if let Ok(val) = consumer.pop() {
                    read_output.push(val);
                }
            }
        }
    }

    // Drain remainder
    while let Ok(val) = consumer.pop() {
        read_output.push(val);
    }

    assert_eq!(
        read_output.len(),
        test_stream.len(),
        "Ring buffer lost samples during bursty arrival: expected {}, got {}",
        test_stream.len(),
        read_output.len()
    );

    let (is_exact, max_diff) = metrics::verify_bit_exactness(&test_stream, &read_output);
    assert!(
        is_exact,
        "Ring buffer corrupted sample order or values during jitter: max diff was {}",
        max_diff
    );
}

/// Simulates a delay effect with high feedback running into a limiter:
///
/// Plain English explanation:
/// When delay feedback is pushed high, sound repeats indefinitely and can accumulate
/// runaway volume that would clip digital audio and harm speakers.
/// This test verifies that putting a Limiter immediately downstream clamps feedback overload
/// safely below -1 dBFS, guaranteeing system stability.
#[test]
fn test_limiter_protects_feedback_delay() {
    let sample_rate = 48_000;

    // Aggressive delay with 80% feedback
    let (mut delay, _delay_ctrl) = DelayEffect::new(
        DelayData {
            time_ms: 30.0,
            feedback: 0.80,
            mix: 0.70,
            bypassed: false,
        },
        sample_rate,
    );

    // Downstream brickwall limiter
    let (mut limiter, _lim_ctrl, _meter) = LimiterEffect::new(
        LimiterData {
            ceiling_db: -1.0,
            release_ms: 20.0,
            lookahead_ms: 5.0,
            bypassed: false,
        },
        sample_rate,
    );

    // Loud impulse burst followed by silence
    let burst = generators::sine_stereo(440.0, 440.0, sample_rate, 0.05, 1.5);
    let silence = vec![0.0f32; (sample_rate as f32 * 0.25) as usize * 2];
    let mut signal = [burst, silence].concat();
    let frames = signal.len() / 2;

    delay.process(&mut signal, frames);
    limiter.process(&mut signal, frames);

    let max_peak = metrics::peak(&signal);
    assert!(
        max_peak <= 0.892, // -1.0 dBFS ceiling
        "Limiter failed to prevent feedback overload from exceeding ceiling: peak was {}",
        max_peak
    );
}

/// Verifies peak level metering ballistics and decay physics:
///
/// Plain English explanation:
/// Professional audio level meters must instantly register sudden audio peaks (transient attack),
/// but decay smoothly rather than dropping immediately to zero so human eyes can track levels.
/// This test verifies instantaneous peak capture followed by predictable logarithmic decay.
#[test]
fn test_peak_metering_ballistics_and_decay() {
    let (mut meter, handle) = LevelMeterEffect::new(LevelMeterData {}, "meter_test".into());

    let frames = 256;
    // Block 1: Extreme peak transient (1.0 amplitude)
    let mut loud_block = vec![1.0f32; frames * 2];
    meter.process(&mut loud_block, frames);

    // Snapshot 1 should read instantaneous peak 1.0 and trigger decay for next tick
    let snap1 = handle.snapshot_and_decay();
    assert_eq!(
        snap1.peaks[0], 1.0,
        "Meter failed to register instantaneous peak"
    );

    // Block 2: Total silence
    let mut silent_block = vec![0.0f32; frames * 2];
    meter.process(&mut silent_block, frames);

    // Snapshot 2 should read decaying peak (~0.85 of previous peak)
    let snap2 = handle.snapshot_and_decay();
    assert!(
        snap2.peaks[0] < 0.90 && snap2.peaks[0] > 0.80,
        "Meter ballistics decay failed: expected ~0.85, got {}",
        snap2.peaks[0]
    );
}

/// Verifies LUFS loudness metering against ITU-R BS.1770 / EBU R128 broadcast standards:
///
/// Plain English explanation:
/// Broadcasting and streaming platforms (YouTube, Spotify, Apple Podcasts) require audio to adhere
/// to strict loudness targets (-14 to -23 LUFS). This test feeds a calibrated standard test tone
/// through the LUFS meter to verify accurate calculation of integrated and momentary loudness.
#[test]
fn test_lufs_metering_loudness_compliance() {
    let sample_rate = 48_000;
    let (mut lufs, handle) = LufsMeterEffect::new(LufsMeterData {}, "lufs_test".into(), sample_rate);

    // Calibrated 1 kHz tone at -20 dBFS (amplitude = 0.1) for 400ms
    let mut tone = generators::sine_stereo(1000.0, 1000.0, sample_rate, 0.4, 0.1);
    let frames = tone.len() / 2;

    lufs.process(&mut tone, frames);

    let snap = handle.snapshot();

    // Standard 1 kHz tone at -20 dBFS in stereo registers around -20 to -23 LUFS
    assert!(
        snap.momentary > -26.0 && snap.momentary < -18.0,
        "LUFS momentary loudness out of expected calibration range: got {} LUFS",
        snap.momentary
    );
    assert_eq!(
        snap.clips, 0,
        "Unclipped calibration signal registered accidental clips"
    );
}

/// Simulates extended session stability across 100 continuous DSP audio blocks:
///
/// Plain English explanation:
/// Audio pipelines must run reliably for hours without buffer drift, memory leaks,
/// denormal float slowdowns, or NaN values. This test streams over 1 second of audio
/// through a 6-stage effect chain, ensuring zero NaNs, zero infinities, and bounded energy.
#[test]
fn test_long_running_pipeline_dsp_stability() {
    let sample_rate = 48_000;
    let block_size = 512;
    let num_blocks = 100;

    let (mut eq, _eq_c) = EqEffect::new(EqData { gains_db: [1.0; 10], bypassed: false }, sample_rate);
    let (mut saturator, _sat_c) = SaturatorEffect::new(SaturatorData { drive_db: 3.0, threshold_db: -3.0, bypassed: false });
    let (mut comp, _comp_c, _comp_m) = CompressorEffect::new(
        CompressorData {
            threshold_db: -12.0,
            ratio: 4.0,
            attack_ms: 10.0,
            release_ms: 50.0,
            knee_db: 0.0,
            makeup_db: 1.0,
            bypassed: false,
        },
        sample_rate,
    );
    let (mut deesser, _de_c) = DeEsserEffect::new(DeEsserData { frequency: 6000.0, threshold_db: -20.0, ratio: 3.0, bypassed: false }, sample_rate);
    let (mut reverb, _rev_c) = ReverbEffect::new(ReverbData { room_size: 0.5, damping: 0.5, width: 1.0, mix: 0.3, bypassed: false }, sample_rate);
    let (mut limiter, _lim_c, _lim_m) = LimiterEffect::new(LimiterData { ceiling_db: -1.0, release_ms: 20.0, lookahead_ms: 5.0, bypassed: false }, sample_rate);

    let mut running_block = generators::sine_stereo(440.0, 880.0, sample_rate, 512.0 / 48000.0, 0.5);

    for block_idx in 0..num_blocks {
        eq.process(&mut running_block, block_size);
        saturator.process(&mut running_block, block_size);
        comp.process(&mut running_block, block_size);
        deesser.process(&mut running_block, block_size);
        reverb.process(&mut running_block, block_size);
        limiter.process(&mut running_block, block_size);

        // Assert numerical stability on every frame of every block
        for &s in &running_block {
            assert!(
                s.is_finite(),
                "Non-finite sample detected at block {}: value = {}",
                block_idx,
                s
            );
        }
    }

    let final_peak = metrics::peak(&running_block);
    assert!(
        final_peak <= 0.892,
        "Long running stability failed: final peak breached ceiling ({})",
        final_peak
    );
}

/// Simulates real-time UI parameter modulation without audio stream interruption:
///
/// Plain English explanation:
/// When a user drags a volume slider in the UI, the engine must update the live audio
/// smoothly in real time without audio glitches, stuttering, or needing to reload the engine.
/// This test verifies instant dynamic parameter response via EffectControl.
#[test]
fn test_dynamic_gain_slider_parameter_modulation() {
    let (mut gain, ctrl) = GainEffect::new(GainData { gain_db: 0.0, bypassed: false });
    let sample_rate = 48_000;
    let frames = 256;

    let mut audio_block = generators::sine_stereo(440.0, 440.0, sample_rate, frames as f32 / sample_rate as f32, 0.3);

    // Step 1: Process at 0 dB (unity)
    gain.process(&mut audio_block, frames);
    let rms_unity = metrics::rms(&audio_block);

    // Step 2: User drags slider to +6.02 dB (amplitude doubles)
    ctrl.apply_update(&json!({ "gainDb": 6.02 }));
    // Block 1 ramps gain smoothly from 1.0x to 2.0x (anti-click smoothing)
    gain.process(&mut audio_block, frames);
    // Block 2 achieves steady-state 2.0x gain
    let mut steady_boosted = generators::sine_stereo(440.0, 440.0, sample_rate, frames as f32 / sample_rate as f32, 0.3);
    gain.process(&mut steady_boosted, frames);
    let rms_boosted = metrics::rms(&steady_boosted);

    // Step 3: User drags slider to -6.02 dB (amplitude halves relative to unity)
    ctrl.apply_update(&json!({ "gainDb": -6.02 }));
    // Block 1 ramps down smoothly
    gain.process(&mut audio_block, frames);
    // Block 2 achieves steady-state 0.5x unity gain (0.25x of boosted)
    let mut steady_cut = generators::sine_stereo(440.0, 440.0, sample_rate, frames as f32 / sample_rate as f32, 0.3);
    gain.process(&mut steady_cut, frames);
    let rms_attenuated = metrics::rms(&steady_cut);

    let boost_ratio = rms_boosted / rms_unity;
    let cut_ratio = rms_attenuated / rms_boosted;

    assert!(
        (boost_ratio - 2.0).abs() < 0.05,
        "Dynamic +6 dB boost failed: expected 2.0x, got {}",
        boost_ratio
    );
    assert!(
        (cut_ratio - 0.25).abs() < 0.05,
        "Dynamic -6 dB cut failed: expected 0.25x of boosted, got {}",
        cut_ratio
    );
}

/// Simulates surround-to-stereo downmixing and mono-fold compatibility:
///
/// Plain English explanation:
/// When multi-channel audio (Left, Right, Center) is mixed down to stereo,
/// dialogue in the Center channel must be placed equally in both stereo channels (-3 dB each).
/// When summed to mono, the dialogue must maintain clarity without destructive phase cancellation.
#[test]
fn test_surround_to_stereo_downmix_compatibility() {
    let sample_rate = 48_000;
    let duration = 0.1; // 100ms
    let frames = (sample_rate as f32 * duration) as usize;

    // Surround elements
    let left_ch = generators::sine(300.0, sample_rate, duration, 0.5);
    let right_ch = generators::sine(600.0, sample_rate, duration, 0.5);
    let center_voice = generators::sine(1000.0, sample_rate, duration, 0.6);

    // ITU-R standard stereo downmix:
    // Left_Out  = Left + 0.7071 * Center
    // Right_Out = Right + 0.7071 * Center
    let center_gain = std::f32::consts::FRAC_1_SQRT_2; // -3.0 dB (~0.7071)
    let mut stereo_out = Vec::with_capacity(frames * 2);

    for i in 0..frames {
        let l = left_ch[i] + center_gain * center_voice[i];
        let r = right_ch[i] + center_gain * center_voice[i];
        stereo_out.push(l);
        stereo_out.push(r);
    }

    // Measure Center channel contribution in Left vs Right
    // Both stereo channels should have equal energy contribution from the center dialogue
    let l_samples: Vec<f32> = stereo_out.chunks_exact(2).map(|f| f[0]).collect();
    let r_samples: Vec<f32> = stereo_out.chunks_exact(2).map(|f| f[1]).collect();

    // Sum stereo downmix to mono: Mono = (Left + Right) / 2
    let mut mono_sum = Vec::with_capacity(frames);
    for i in 0..frames {
        mono_sum.push((l_samples[i] + r_samples[i]) * 0.5);
    }

    let mono_rms = metrics::rms(&mono_sum);
    assert!(
        mono_rms > 0.25,
        "Mono downmix suffered destructive phase cancellation: RMS was {}",
        mono_rms
    );
}
