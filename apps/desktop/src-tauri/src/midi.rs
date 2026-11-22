//! MIDI input for the control layer. One thread watches the system's MIDI
//! inputs, keeps a connection open to each, and forwards note and CC messages
//! to the webview, where Key Input nodes match them. Audio never runs here.

use std::collections::HashMap;
use std::time::Duration;

use midir::{Ignore, MidiInput, MidiInputConnection};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tracing::{info, warn};
use ts_rs::TS;

const MIDI_EVENT: &str = "midi://message";
const CLIENT_NAME: &str = "Splitwave";
/// Devices plugged in later are picked up on the next scan.
const SCAN_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum MidiKind {
    Note,
    Cc,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MidiMessage {
    pub device: String,
    /// 1..=16, as hardware labels it.
    pub channel: u8,
    pub kind: MidiKind,
    pub number: u8,
    /// Velocity for a note (0 when released), the value for a CC.
    pub value: u8,
}

/// Note on, note off and control change; everything else is not a control.
fn parse(device: &str, bytes: &[u8]) -> Option<MidiMessage> {
    let [status, number, value] = *bytes else {
        return None;
    };
    let channel = (status & 0x0F) + 1;
    let (kind, value) = match status & 0xF0 {
        0x80 => (MidiKind::Note, 0),
        0x90 => (MidiKind::Note, value),
        0xB0 => (MidiKind::Cc, value),
        _ => return None,
    };
    Some(MidiMessage {
        device: device.to_owned(),
        channel,
        kind,
        number,
        value,
    })
}

pub fn start(app: AppHandle) {
    let spawned = std::thread::Builder::new()
        .name("midi-input".into())
        .spawn(move || watch(app));
    if let Err(error) = spawned {
        warn!(%error, "midi input thread failed to start");
    }
}

fn watch(app: AppHandle) {
    let mut open: HashMap<String, MidiInputConnection<()>> = HashMap::new();
    loop {
        match MidiInput::new(CLIENT_NAME) {
            Ok(probe) => {
                let ports = probe.ports();
                let names: Vec<(String, _)> = ports
                    .into_iter()
                    .filter_map(|p| probe.port_name(&p).ok().map(|n| (n, p)))
                    .collect();
                open.retain(|name, _| names.iter().any(|(n, _)| n == name));
                for (name, port) in names {
                    if open.contains_key(&name) {
                        continue;
                    }
                    if let Some(conn) = connect(&app, &name, &port) {
                        info!(device = %name, "midi input connected");
                        open.insert(name, conn);
                    }
                }
            }
            Err(error) => warn!(%error, "midi client unavailable"),
        }
        std::thread::sleep(SCAN_INTERVAL);
    }
}

fn connect(
    app: &AppHandle,
    name: &str,
    port: &midir::MidiInputPort,
) -> Option<MidiInputConnection<()>> {
    let mut input = MidiInput::new(CLIENT_NAME).ok()?;
    input.ignore(Ignore::All);
    let app = app.clone();
    let device = name.to_owned();
    input
        .connect(
            port,
            name,
            move |_, bytes, _| {
                if let Some(msg) = parse(&device, bytes) {
                    let _ = app.emit(MIDI_EVENT, msg);
                }
            },
            (),
        )
        .map_err(|error| warn!(device = %name, %error, "midi input connect failed"))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_on_and_off() {
        let on = parse("d", &[0x91, 60, 100]).unwrap();
        assert_eq!(
            (on.channel, on.kind, on.number, on.value),
            (2, MidiKind::Note, 60, 100)
        );
        assert_eq!(parse("d", &[0x80, 60, 64]).unwrap().value, 0);
        assert_eq!(parse("d", &[0x90, 60, 0]).unwrap().value, 0);
    }

    #[test]
    fn control_change() {
        let cc = parse("d", &[0xBF, 64, 127]).unwrap();
        assert_eq!(
            (cc.channel, cc.kind, cc.number, cc.value),
            (16, MidiKind::Cc, 64, 127)
        );
    }

    #[test]
    fn other_messages_are_ignored() {
        assert!(parse("d", &[0xE0, 0, 64]).is_none());
        assert!(parse("d", &[0xF8]).is_none());
    }
}
