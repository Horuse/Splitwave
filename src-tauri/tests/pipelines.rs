//! Whole pipelines, run end to end on virtual devices: inputs, effects,
//! analyzers and outputs combined as people wire them in the editor. Each
//! scenario runs in real time through the app's own engine and checks that
//! every worker kept up, that no source ran dry and that the sound came out
//! unbroken. No sound hardware is needed.

mod common;

use common::*;
use serde_json::json;

/// The kinds of input a graph can start from.
#[derive(Clone, Copy)]
enum Input {
    WavFile,
    Wav44File,
    FlacFile,
    Mp3File,
    Mic,
    Mic44,
    Mic96,
    Interface,
    System,
    App,
}

/// Adds `input` to `graph` as node `id`.
fn with_input(graph: Graph, id: &str, input: Input) -> Graph {
    match input {
        Input::WavFile => graph.node(
            id,
            "audioFile",
            audio_file(&tone_file("tone.wav", wav(), RATE)),
        ),
        Input::Wav44File => graph.node(
            id,
            "audioFile",
            audio_file(&tone_file("tone-44k.wav", wav(), 44_100)),
        ),
        Input::FlacFile => graph.node(
            id,
            "audioFile",
            audio_file(&tone_file("tone.flac", flac(), RATE)),
        ),
        Input::Mp3File => graph.node(
            id,
            "audioFile",
            audio_file(&tone_file("tone.mp3", mp3(), RATE)),
        ),
        Input::Mic => graph.node(id, "microphone", mic("mic")),
        Input::Mic44 => graph.node(id, "microphone", mic("mic-44k")),
        Input::Mic96 => graph.node(id, "microphone", mic("mic-96k")),
        Input::Interface => graph.node(id, "microphone", mic("interface")),
        Input::System => graph.node(
            id,
            "systemAudio",
            json!({ "excludeCurrentApp": true, "volume": 1.0 }),
        ),
        Input::App => graph.node(
            id,
            "appAudio",
            json!({ "bundleId": "com.example.player", "volume": 1.0 }),
        ),
    }
}

/// Input straight into a speaker.
fn direct(input: Input, buffer: u32) {
    let graph = with_input(Graph::new(buffer), "in", input)
        .node("s", "speaker", speaker("out"))
        .edge("in", "s");
    run(&graph, &[("out", 2)]).assert_clean("out", 0.05);
}

/// Input through a short chain, with a meter reading the input on the side.
fn chain_with_meter(input: Input) {
    let graph = with_input(Graph::new(64), "in", input)
        .node("g", "gain", effect("gain"))
        .node("eq", "eq", effect("eq"))
        .node("l", "limiter", effect("limiter"))
        .node("m", "levelMeter", effect("levelMeter"))
        .node("s", "speaker", speaker("out"))
        .chain(&["in", "g", "eq", "l", "s"])
        .edge("in", "m");
    run(&graph, &[("out", 2)]).assert_clean("out", 0.05);
}

/// Input played on two speakers at once.
fn two_speakers(input: Input) {
    let graph = with_input(Graph::new(128), "in", input)
        .node("g", "gain", effect("gain"))
        .node("s1", "speaker", speaker("out"))
        .node("s2", "speaker", speaker("out-2"))
        .chain(&["in", "g", "s1"])
        .edge("g", "s2");
    let m = run(&graph, &[("out", 2), ("out-2", 2)]);
    m.assert_clean("out", 0.05);
    m.assert_tone("out-2", 0, 0.05);
}

/// Input on a speaker running at another rate than the pipeline.
fn other_rate_speaker(input: Input) {
    let graph = with_input(Graph::new(256), "in", input)
        .node("s", "speaker", speaker("out-44k"))
        .edge("in", "s");
    run(&graph, &[("out-44k", 2)]).assert_clean("out-44k", 0.05);
}

macro_rules! per_input {
    ($($input:ident => $direct256:ident, $direct32:ident, $chain:ident, $two:ident, $rate:ident;)*) => {
        $(
            #[test]
            fn $direct256() {
                let _g = serial();
                direct(Input::$input, 256);
            }

            #[test]
            fn $direct32() {
                let _g = serial();
                direct(Input::$input, 32);
            }

            #[test]
            fn $chain() {
                let _g = serial();
                chain_with_meter(Input::$input);
            }

            #[test]
            fn $two() {
                let _g = serial();
                two_speakers(Input::$input);
            }

            #[test]
            fn $rate() {
                let _g = serial();
                other_rate_speaker(Input::$input);
            }
        )*
    };
}

per_input! {
    WavFile => wav_file_to_speaker, wav_file_to_speaker_at_32, wav_file_through_chain_with_meter,
        wav_file_on_two_speakers, wav_file_on_44k_speaker;
    Wav44File => wav_44k_file_to_speaker, wav_44k_file_to_speaker_at_32,
        wav_44k_file_through_chain_with_meter, wav_44k_file_on_two_speakers,
        wav_44k_file_on_44k_speaker;
    FlacFile => flac_file_to_speaker, flac_file_to_speaker_at_32, flac_file_through_chain_with_meter,
        flac_file_on_two_speakers, flac_file_on_44k_speaker;
    Mp3File => mp3_file_to_speaker, mp3_file_to_speaker_at_32, mp3_file_through_chain_with_meter,
        mp3_file_on_two_speakers, mp3_file_on_44k_speaker;
    Mic => mic_to_speaker, mic_to_speaker_at_32, mic_through_chain_with_meter,
        mic_on_two_speakers, mic_on_44k_speaker;
    Mic44 => mic_44k_to_speaker, mic_44k_to_speaker_at_32, mic_44k_through_chain_with_meter,
        mic_44k_on_two_speakers, mic_44k_on_44k_speaker;
    Mic96 => mic_96k_to_speaker, mic_96k_to_speaker_at_32, mic_96k_through_chain_with_meter,
        mic_96k_on_two_speakers, mic_96k_on_44k_speaker;
    Interface => interface_to_speaker, interface_to_speaker_at_32,
        interface_through_chain_with_meter, interface_on_two_speakers, interface_on_44k_speaker;
    System => system_audio_to_speaker, system_audio_to_speaker_at_32,
        system_audio_through_chain_with_meter, system_audio_on_two_speakers,
        system_audio_on_44k_speaker;
    App => app_audio_to_speaker, app_audio_to_speaker_at_32, app_audio_through_chain_with_meter,
        app_audio_on_two_speakers, app_audio_on_44k_speaker;
}

/// One effect between a microphone and a speaker, at its default settings.
fn single_effect(kind: &str) {
    let graph = Graph::new(64)
        .node("m", "microphone", mic("mic"))
        .node("fx", kind, effect(kind))
        .node("s", "speaker", speaker("out"))
        .chain(&["m", "fx", "s"]);
    let m = run(&graph, &[("out", 2)]);
    m.assert_real_time();
    m.assert_no_dropouts();
    // A steady tone is exactly what a noise suppressor removes.
    if kind != "noiseSuppressor" {
        m.assert_tone("out", 0, 0.03);
    }
}

macro_rules! per_effect {
    ($($name:ident => $kind:literal;)*) => {
        $(
            #[test]
            fn $name() {
                let _g = serial();
                single_effect($kind);
            }
        )*
    };
}

per_effect! {
    effect_gain => "gain";
    effect_mute => "mute";
    effect_channel_balance => "channelBalance";
    effect_saturator => "saturator";
    effect_eq => "eq";
    effect_level_meter => "levelMeter";
    effect_lufs_meter => "lufsMeter";
    effect_waveform => "waveform";
    effect_spectrum => "spectrum";
    effect_limiter => "limiter";
    effect_compressor => "compressor";
    effect_noise_gate => "noiseGate";
    effect_delay => "delay";
    effect_reverb => "reverb";
    effect_noise_suppressor => "noiseSuppressor";
    effect_declick => "declick";
    effect_de_esser => "deEsser";
}

/// A file through an effect to a speaker, with an analyzer reading the
/// effect's output: the analyzer runs on the monitor, the speaker on its
/// device, both from one file.
fn file_effect_analyzer(effect_kind: &str, analyzer: &str) {
    let graph = with_input(Graph::new(64), "f", Input::WavFile)
        .node("fx", effect_kind, effect(effect_kind))
        .node("an", analyzer, effect(analyzer))
        .node("s", "speaker", speaker("out"))
        .chain(&["f", "fx", "s"])
        .edge("fx", "an");
    run(&graph, &[("out", 2)]).assert_clean("out", 0.03);
}

macro_rules! per_effect_analyzer {
    ($($name:ident => $fx:literal, $an:literal;)*) => {
        $(
            #[test]
            fn $name() {
                let _g = serial();
                file_effect_analyzer($fx, $an);
            }
        )*
    };
}

per_effect_analyzer! {
    file_gain_waveform => "gain", "waveform";
    file_gain_spectrum => "gain", "spectrum";
    file_gain_lufs => "gain", "lufsMeter";
    file_eq_waveform => "eq", "waveform";
    file_eq_spectrum => "eq", "spectrum";
    file_eq_lufs => "eq", "lufsMeter";
    file_compressor_waveform => "compressor", "waveform";
    file_compressor_spectrum => "compressor", "spectrum";
    file_compressor_lufs => "compressor", "lufsMeter";
    file_reverb_waveform => "reverb", "waveform";
    file_reverb_spectrum => "reverb", "spectrum";
    file_reverb_lufs => "reverb", "lufsMeter";
    file_delay_waveform => "delay", "waveform";
    file_delay_spectrum => "delay", "spectrum";
    file_delay_lufs => "delay", "lufsMeter";
}

/// What a recording wrote, as frames of its first channel.
fn read_wav(path: &std::path::Path) -> (Vec<f32>, u32) {
    let mut reader = hound::WavReader::open(path).expect("recording opens");
    let spec = reader.spec();
    let channels = spec.channels as usize;
    let samples: Vec<f32> = reader
        .samples::<f32>()
        .map(|s| s.expect("sample"))
        .collect();
    (
        samples.into_iter().step_by(channels).collect(),
        spec.sample_rate,
    )
}

fn recording(name: &str, input: Input, also_speaker: bool) {
    let path = temp_path(name);
    let _ = std::fs::remove_file(&path);
    let mut graph = with_input(Graph::new(256), "in", input)
        .node(
            "rec",
            "fileRecording",
            json!({
                "filePath": path.to_string_lossy(),
                "format": wav(),
                "mode": "overwrite",
                "channels": 2,
                "sampleRate": RATE,
            }),
        )
        .edge("in", "rec");
    if also_speaker {
        graph = graph.node("s", "speaker", speaker("out")).edge("in", "s");
    }
    let speakers: &[(&str, usize)] = if also_speaker { &[("out", 2)] } else { &[] };
    let mut m = run(&graph, speakers);
    m.assert_real_time();
    m.assert_no_dropouts();
    if also_speaker {
        m.assert_tone("out", 0, 0.05);
    }
    m.rig.stop();
    let (frames, rate) = read_wav(&path);
    let seconds = frames.len() as f64 / rate as f64;
    let ran = (WARMUP + m.window).as_secs_f64();
    assert!(
        seconds > ran * 0.7 && seconds < ran * 1.3,
        "recorded {seconds:.2} s of {ran:.2} s"
    );
    // Past the opening, the tone is whole: no run of silence.
    let body = &frames[frames.len() / 4..frames.len() * 3 / 4];
    let mut run = 0;
    let mut worst = 0;
    for s in body {
        run = if s.abs() < 1e-4 { run + 1 } else { 0 };
        worst = worst.max(run);
    }
    assert!(
        worst < rate as usize / 1000,
        "recording holds {worst} frames of silence"
    );
}

#[test]
fn record_mic() {
    let _g = serial();
    recording("mic.wav", Input::Mic, false);
}

#[test]
fn record_file() {
    let _g = serial();
    recording("file.wav", Input::FlacFile, false);
}

#[test]
fn record_mic_while_playing_it() {
    let _g = serial();
    recording("mic-and-speaker.wav", Input::Mic, true);
}

#[test]
fn record_system_audio_while_playing_it() {
    let _g = serial();
    recording("system-and-speaker.wav", Input::System, true);
}

/// A microphone sent over the network to this machine and played back.
fn network_loopback(port: u16, codec: &str) {
    let graph = Graph::new(128)
        .node("m", "microphone", mic("mic"))
        .node(
            "tx",
            "netSender",
            json!({
                "targetIp": "127.0.0.1",
                "port": port,
                "channels": 1,
                "codec": codec,
                "opusBitrate": 96000,
                "opusApplication": "audio",
                "sampleRate": null,
            }),
        )
        .node("rx", "netReceiver", json!({ "port": port, "channels": 1 }))
        .node("s", "speaker", speaker("out"))
        .edge_handles("m", None, "tx", Some("ch1"))
        .edge_handles("rx", Some("ch1"), "s", None);
    let mut rig = splitwave_lib::testkit::Rig::new(devices());
    rig.apply(graph.json()).expect("graph starts");
    // The receiver primes from the first packets before playing.
    std::thread::sleep(std::time::Duration::from_millis(700));
    let m = measure(rig, &[("out", 2)]);
    m.assert_real_time();
    // Opus is lossy; the tone still arrives whole and near its level.
    m.assert_tone("out", 0, 0.03);
}

#[test]
fn network_pcm_loopback() {
    let _g = serial();
    network_loopback(47_311, "pcm-f32");
}

#[test]
fn network_opus_loopback() {
    let _g = serial();
    network_loopback(47_312, "opus");
}

/// Starts on `from`, moves to `to` while playing, then measures `to`.
fn edit_while_playing(from: Graph, to: Graph) {
    let mut rig = splitwave_lib::testkit::Rig::new(devices());
    rig.apply(from.json()).expect("first graph starts");
    std::thread::sleep(WARMUP);
    rig.apply(to.json()).expect("edit applies");
    measure(rig, &[("out", 2)]).assert_clean("out", 0.03);
}

fn mic_to_speaker_graph() -> Graph {
    Graph::new(64)
        .node("m", "microphone", mic("mic"))
        .node("s", "speaker", speaker("out"))
        .edge("m", "s")
}

#[test]
fn edit_adds_an_effect() {
    let _g = serial();
    let to = Graph::new(64)
        .node("m", "microphone", mic("mic"))
        .node("g", "gain", effect("gain"))
        .node("s", "speaker", speaker("out"))
        .chain(&["m", "g", "s"]);
    edit_while_playing(mic_to_speaker_graph(), to);
}

#[test]
fn edit_removes_an_effect() {
    let _g = serial();
    let from = Graph::new(64)
        .node("m", "microphone", mic("mic"))
        .node("c", "compressor", effect("compressor"))
        .node("s", "speaker", speaker("out"))
        .chain(&["m", "c", "s"]);
    edit_while_playing(from, mic_to_speaker_graph());
}

#[test]
fn edit_adds_an_analyzer() {
    let _g = serial();
    let to = mic_to_speaker_graph()
        .node("w", "waveform", effect("waveform"))
        .edge("m", "w");
    edit_while_playing(mic_to_speaker_graph(), to);
}

#[test]
fn edit_adds_an_analyzer_to_a_playing_file() {
    let _g = serial();
    let from = with_input(Graph::new(32), "f", Input::WavFile)
        .node("s", "speaker", speaker("out"))
        .edge("f", "s");
    let to = with_input(Graph::new(32), "f", Input::WavFile)
        .node("s", "speaker", speaker("out"))
        .node("sp", "spectrum", effect("spectrum"))
        .edge("f", "s")
        .edge("f", "sp");
    edit_while_playing(from, to);
}

#[test]
fn edit_adds_a_second_speaker() {
    let _g = serial();
    let to = mic_to_speaker_graph()
        .node("s2", "speaker", speaker("out-2"))
        .edge("m", "s2");
    edit_while_playing(mic_to_speaker_graph(), to);
}

#[test]
fn edit_changes_the_buffer() {
    let _g = serial();
    let to = Graph::new(256)
        .node("m", "microphone", mic("mic"))
        .node("s", "speaker", speaker("out"))
        .edge("m", "s");
    edit_while_playing(mic_to_speaker_graph(), to);
}

#[test]
fn mic_and_file_mixed() {
    let _g = serial();
    let graph = with_input(Graph::new(64), "f", Input::WavFile)
        .node("m", "microphone", mic("mic"))
        .node("s", "speaker", speaker("out"))
        .edge("f", "s")
        .edge("m", "s");
    run(&graph, &[("out", 2)]).assert_clean("out", 0.05);
}

#[test]
fn system_and_app_mixed_through_a_compressor() {
    let _g = serial();
    let graph = Graph::new(128)
        .node(
            "sys",
            "systemAudio",
            json!({ "excludeCurrentApp": true, "volume": 1.0 }),
        )
        .node(
            "app",
            "appAudio",
            json!({ "bundleId": "com.example.player", "volume": 1.0 }),
        )
        .node("c", "compressor", effect("compressor"))
        .node("s", "speaker", speaker("out"))
        .edge("sys", "c")
        .edge("app", "c")
        .edge("c", "s");
    run(&graph, &[("out", 2)]).assert_clean("out", 0.05);
}

#[test]
fn stereo_file_on_a_six_channel_speaker() {
    let _g = serial();
    let graph = with_input(Graph::new(128), "f", Input::WavFile)
        .node("s", "speaker", speaker("out-6ch"))
        .edge("f", "s");
    run(&graph, &[("out-6ch", 6)]).assert_clean("out-6ch", 0.05);
}

#[test]
fn four_channel_interface_through_reverb() {
    let _g = serial();
    let graph = Graph::new(128)
        .node("i", "microphone", mic("interface"))
        .node("r", "reverb", effect("reverb"))
        .node("s", "speaker", speaker("out"))
        .chain(&["i", "r", "s"]);
    run(&graph, &[("out", 2)]).assert_clean("out", 0.05);
}

#[test]
fn analyzers_only_run_in_real_time() {
    let _g = serial();
    // No speaker at all: the monitor alone drives the graph.
    let graph = Graph::new(32)
        .node("m", "microphone", mic("mic"))
        .node("lm", "levelMeter", effect("levelMeter"))
        .node("w", "waveform", effect("waveform"))
        .edge("m", "lm")
        .edge("m", "w");
    let m = run(&graph, &[]);
    m.assert_real_time();
    m.assert_no_dropouts();
}
