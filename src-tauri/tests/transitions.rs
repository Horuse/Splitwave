//! Edits made while audio plays, heard through a speaker. An edit that does
//! not touch what the speaker plays is inaudible there. One that does may dip
//! under a fade, but never steps the waveform, and leaves nothing to correct
//! afterwards. The microphone is `real-mic`: its own clock and whole IO
//! buffers, as CoreAudio delivers one.

mod common;

use std::time::Duration;

use common::*;
use serde_json::{json, Value};

const MIC: &str = "real-mic";

/// What the speaker's first channel did around an edit, times in ms after it.
struct Heard {
    /// Largest departure from the tone's own continuation, sample to sample.
    worst_step: f32,
    /// Where it departed by more than a fade's bend.
    breaks: Vec<f64>,
    /// 5 ms windows whose level is off the tone's by more than 10%.
    off: Vec<f64>,
}

impl Heard {
    /// Nothing at all changed in what the speaker played.
    fn assert_unheard(&self, what: &str) {
        assert!(
            self.breaks.is_empty() && self.off.is_empty(),
            "{what}: heard at {:?} ms (breaks) / {:?} ms (level)",
            self.breaks,
            self.off
        );
    }

    /// At most a dip under a fade, right after the edit.
    fn assert_smooth(&self, what: &str) {
        assert!(
            self.worst_step < 0.03,
            "{what}: the waveform stepped by {}",
            self.worst_step
        );
        let late: Vec<f64> = self
            .breaks
            .iter()
            .chain(&self.off)
            .copied()
            .filter(|t| *t > 400.0)
            .collect();
        assert!(late.is_empty(), "{what}: still moving at {late:?} ms");
    }
}

fn listen(x: &[f32], edit_at: usize) -> Heard {
    let ms = |i: usize| (i as f64 - edit_at as f64) * 1000.0 / RATE as f64;
    // A pure tone continues as x[n+1] = 2 cos(w) x[n] - x[n-1].
    let k = 2.0 * (std::f32::consts::TAU * TONE_HZ / RATE as f32).cos();
    let mut heard = Heard {
        worst_step: 0.0,
        breaks: Vec::new(),
        off: Vec::new(),
    };
    for i in 1..x.len() - 1 {
        let r = (x[i + 1] - k * x[i] + x[i - 1]).abs();
        heard.worst_step = heard.worst_step.max(r);
        if r > 0.01 {
            heard.breaks.push(ms(i));
        }
    }
    let nominal = TONE_AMP / std::f32::consts::SQRT_2;
    let win = RATE as usize / 200;
    for (n, c) in x.chunks_exact(win).enumerate() {
        let rms = (c.iter().map(|s| s * s).sum::<f32>() / win as f32).sqrt();
        if (rms / nominal - 1.0).abs() > 0.1 {
            heard.off.push(ms(n * win));
        }
    }
    heard
}

/// Plays `from` until it has settled, edits it into `to`, and listens to
/// speaker `out` from just before the edit.
fn edit(from: Graph, to: Graph, settle: Duration) -> Heard {
    let mut rig = splitwave_lib::testkit::Rig::new(devices());
    rig.apply(from.json()).expect("first graph starts");
    std::thread::sleep(settle);
    rig.clear_played();
    std::thread::sleep(Duration::from_millis(200));
    let edit_at = rig.played("out").len() / 2;
    rig.apply(to.json()).expect("edit applies");
    std::thread::sleep(Duration::from_millis(1200));
    let played = rig.played("out");
    let ch0: Vec<f32> = played.iter().step_by(2).copied().collect();
    listen(&ch0, edit_at)
}

/// Past a captured source's one startup correction.
const SETTLED: Duration = Duration::from_millis(1600);
/// Past a network receiver's, which measures over longer windows.
const SETTLED_NET: Duration = Duration::from_millis(2600);

fn mic_speaker() -> Graph {
    Graph::new(64)
        .node("m", "microphone", mic(MIC))
        .node("s", "speaker", speaker("out"))
        .edge("m", "s")
}

fn sender(port: u16, channels: u32, sample_rate: Value, codec: &str) -> Value {
    json!({
        "targetIp": "127.0.0.1",
        "port": port,
        "channels": channels,
        "codec": codec,
        "opusBitrate": 96000,
        "opusApplication": "audio",
        "sampleRate": sample_rate,
    })
}

fn with_sender(sample_rate: Value, codec: &str) -> Graph {
    mic_speaker()
        .node("tx", "netSender", sender(47_410, 2, sample_rate, codec))
        .edge("m", "tx")
}

/// The microphone sent over the network to this machine and played.
fn loopback(port: u16, sample_rate: Value, codec: &str) -> Graph {
    Graph::new(128)
        .node("m", "microphone", mic(MIC))
        .node("tx", "netSender", sender(port, 1, sample_rate, codec))
        .node("rx", "netReceiver", json!({ "port": port, "channels": 1 }))
        .node("s", "speaker", speaker("out"))
        .edge_handles("m", None, "tx", Some("ch1"))
        .edge_handles("rx", Some("ch1"), "s", None)
}

#[test]
fn a_sender_changing_its_rate_is_not_heard_on_the_speaker() {
    let _g = serial();
    edit(
        with_sender(Value::Null, "pcm-f32"),
        with_sender(json!(44_100), "pcm-f32"),
        SETTLED,
    )
    .assert_unheard("sender rate");
}

#[test]
fn a_sender_changing_its_codec_is_not_heard_on_the_speaker() {
    let _g = serial();
    edit(
        with_sender(Value::Null, "pcm-f32"),
        with_sender(Value::Null, "opus"),
        SETTLED,
    )
    .assert_unheard("sender codec");
}

#[test]
fn adding_and_removing_a_sender_is_not_heard() {
    let _g = serial();
    edit(mic_speaker(), with_sender(Value::Null, "pcm-f32"), SETTLED).assert_unheard("add sender");
    edit(with_sender(Value::Null, "pcm-f32"), mic_speaker(), SETTLED)
        .assert_unheard("remove sender");
}

#[test]
fn adding_an_analyzer_is_not_heard() {
    let _g = serial();
    let to = mic_speaker()
        .node("w", "waveform", effect("waveform"))
        .edge("m", "w");
    edit(mic_speaker(), to, SETTLED).assert_unheard("add analyzer");
}

#[test]
fn adding_a_second_speaker_is_not_heard_on_the_first() {
    let _g = serial();
    let to = mic_speaker()
        .node("s2", "speaker", speaker("out-2"))
        .edge("m", "s2");
    edit(mic_speaker(), to, SETTLED).assert_unheard("add second speaker");
}

#[test]
fn starting_a_recording_is_not_heard() {
    let _g = serial();
    let path = temp_path("transition-rec.wav");
    let to = mic_speaker()
        .node(
            "r",
            "fileRecording",
            json!({ "filePath": path.to_string_lossy(), "format": wav(), "sampleRate": RATE }),
        )
        .edge("m", "r");
    edit(mic_speaker(), to, SETTLED).assert_unheard("add recording");
}

#[test]
fn an_effect_on_another_speaker_is_not_heard() {
    let _g = serial();
    let from = mic_speaker()
        .node("s2", "speaker", speaker("out-2"))
        .edge("m", "s2");
    let to = mic_speaker()
        .node("r", "reverb", effect("reverb"))
        .node("s2", "speaker", speaker("out-2"))
        .chain(&["m", "r", "s2"]);
    edit(from, to, SETTLED).assert_unheard("reverb on another speaker");
}

#[test]
fn a_volume_change_neither_rebuilds_nor_steps() {
    let _g = serial();
    let vol = |v: f32| {
        Graph::new(64)
            .node("m", "microphone", json!({ "deviceId": MIC, "volume": v }))
            .node("s", "speaker", speaker("out"))
            .edge("m", "s")
    };
    edit(vol(1.0), vol(0.95), SETTLED).assert_unheard("volume");
}

#[test]
fn adding_an_effect_dips_without_a_step() {
    let _g = serial();
    let to = Graph::new(64)
        .node("m", "microphone", mic(MIC))
        .node("g", "gain", effect("gain"))
        .node("s", "speaker", speaker("out"))
        .chain(&["m", "g", "s"]);
    edit(mic_speaker(), to, SETTLED).assert_smooth("add gain");
}

#[test]
fn a_new_lookahead_fades_in_once_its_delay_has_filled() {
    let _g = serial();
    let limiter = |lookahead: f32| {
        Graph::new(64)
            .node("m", "microphone", mic(MIC))
            .node(
                "l",
                "limiter",
                json!({ "ceilingDb": -0.3, "lookaheadMs": lookahead, "releaseMs": 50 }),
            )
            .node("s", "speaker", speaker("out"))
            .chain(&["m", "l", "s"])
    };
    edit(limiter(5.0), limiter(2.0), SETTLED).assert_smooth("limiter lookahead");
}

#[test]
fn a_new_buffer_reopens_the_speaker_without_correcting_afterwards() {
    let _g = serial();
    let to = Graph::new(256)
        .node("m", "microphone", mic(MIC))
        .node("s", "speaker", speaker("out"))
        .edge("m", "s");
    edit(mic_speaker(), to, SETTLED).assert_smooth("buffer 64 -> 256");
}

#[test]
fn an_app_source_rebuilt_behind_an_effect_dips_without_a_step() {
    let _g = serial();
    let app = |gain: bool| {
        let g = Graph::new(64)
            .node(
                "a",
                "appAudio",
                json!({ "bundleId": "com.example.player", "volume": 1.0 }),
            )
            .node("s", "speaker", speaker("out"));
        if gain {
            g.node("g", "gain", effect("gain")).chain(&["a", "g", "s"])
        } else {
            g.edge("a", "s")
        }
    };
    edit(app(false), app(true), SETTLED).assert_smooth("app audio add gain");
}

#[test]
fn a_receiver_rides_out_its_sender_changing_rate() {
    let _g = serial();
    edit(
        loopback(47_411, Value::Null, "pcm-f32"),
        loopback(47_411, json!(44_100), "pcm-f32"),
        SETTLED_NET,
    )
    .assert_smooth("received rate");
}

#[test]
fn a_receiver_rides_out_its_sender_changing_codec() {
    let _g = serial();
    edit(
        loopback(47_412, Value::Null, "pcm-f32"),
        loopback(47_412, Value::Null, "opus"),
        SETTLED_NET,
    )
    .assert_smooth("received codec");
}

#[test]
fn the_first_start_corrects_its_depth_in_one_soft_cut() {
    let _g = serial();
    let mut rig = splitwave_lib::testkit::Rig::new(devices());
    rig.apply(mic_speaker().json()).expect("graph starts");
    std::thread::sleep(Duration::from_millis(2500));
    let played = rig.played("out");
    let ch0: Vec<f32> = played.iter().step_by(2).copied().collect();
    let first = ch0.iter().position(|s| s.abs() > 1e-3).expect("it plays");
    // The fade-in of the first sound is the only bend at the start.
    let heard = listen(&ch0[first + RATE as usize / 50..], 0);
    assert!(heard.breaks.is_empty(), "breaks at {:?} ms", heard.breaks);
    assert!(
        heard.off.len() <= 3,
        "level off at {:?} ms: more than one cut",
        heard.off
    );
}
