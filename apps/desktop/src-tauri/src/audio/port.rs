//! Graph sockets. The UI names every handle `<signal>:<port>`; this is the one
//! place that reads that format, so the engine only ever sees typed ports.

use std::str::FromStr;

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
#[serde(try_from = "String")]
pub enum Port {
    Audio(AudioPort),
    Control(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AudioPort {
    /// The node's whole bus.
    Main,
    /// One channel, 1-based (`chK`).
    Channel(usize),
    /// The stereo pair starting at 1-based channel A (`stA`).
    Stereo(usize),
    /// A WebRTC peer's mix, or one of its channels counted from zero.
    Peer {
        peer: String,
        channel: Option<usize>,
    },
    /// An effect's sidechain key input.
    Sidechain,
    /// `port` on one output of a branching node, 1-based. Graph validation
    /// resolves it to a port of that output's own node; the engine never sees it.
    Branch { branch: usize, port: Box<AudioPort> },
}

impl Port {
    pub const MAIN: Port = Port::Audio(AudioPort::Main);
}

impl AudioPort {
    /// Width of a per-channel tap; `None` for ports that carry a whole bus.
    pub fn tap_width(&self) -> Option<usize> {
        match self {
            AudioPort::Channel(_) => Some(1),
            AudioPort::Stereo(_) => Some(2),
            _ => None,
        }
    }

    /// The `(channel offset, width)` block this port addresses on a node.
    pub fn route(&self) -> Option<(usize, usize)> {
        match self {
            AudioPort::Channel(k) => Some((k - 1, 1)),
            AudioPort::Stereo(a) => Some((a - 1, 2)),
            _ => None,
        }
    }

    /// How many channels a node needs for this port to exist on it.
    pub fn channel_span(&self) -> Option<usize> {
        self.route().map(|(off, w)| off + w)
    }
}

impl FromStr for Port {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.split_once(':') {
            Some(("audio", port)) => port.parse().map(Port::Audio),
            Some(("control", name)) if !name.is_empty() => Ok(Port::Control(name.to_string())),
            _ => Err(format!("unknown handle {s:?}")),
        }
    }
}

impl TryFrom<String> for Port {
    type Error = String;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl FromStr for AudioPort {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bad = || format!("unknown audio port {s:?}");
        let index = |n: &str| n.parse::<usize>().ok().filter(|&n| n >= 1).ok_or_else(bad);
        match s {
            "main" => return Ok(AudioPort::Main),
            "sidechain" => return Ok(AudioPort::Sidechain),
            _ => {}
        }
        if let Some(rest) = s.strip_prefix("branch") {
            let (branch, port) = rest.split_once(':').ok_or_else(bad)?;
            let port: AudioPort = port.parse()?;
            if matches!(port, AudioPort::Branch { .. }) {
                return Err(bad());
            }
            return Ok(AudioPort::Branch {
                branch: index(branch)?,
                port: Box::new(port),
            });
        }
        if let Some(rest) = s.strip_prefix("peer:") {
            let (peer, channel) = match rest.split_once(':') {
                Some((peer, c)) => (peer, Some(c.parse::<usize>().map_err(|_| bad())?)),
                None => (rest, None),
            };
            if peer.is_empty() {
                return Err(bad());
            }
            return Ok(AudioPort::Peer {
                peer: peer.to_string(),
                channel,
            });
        }
        if let Some(k) = s.strip_prefix("ch") {
            return index(k).map(AudioPort::Channel);
        }
        if let Some(a) = s.strip_prefix("st") {
            return index(a).map(AudioPort::Stereo);
        }
        Err(bad())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn audio(s: &str) -> AudioPort {
        match s.parse::<Port>().expect(s) {
            Port::Audio(p) => p,
            Port::Control(_) => panic!("{s} is not audio"),
        }
    }

    #[test]
    fn parses_audio_ports() {
        assert_eq!(audio("audio:main"), AudioPort::Main);
        assert_eq!(audio("audio:sidechain"), AudioPort::Sidechain);
        assert_eq!(audio("audio:ch3"), AudioPort::Channel(3));
        assert_eq!(audio("audio:st2"), AudioPort::Stereo(2));
        assert_eq!(
            audio("audio:peer:p"),
            AudioPort::Peer {
                peer: "p".into(),
                channel: None
            }
        );
        assert_eq!(
            audio("audio:peer:p:0"),
            AudioPort::Peer {
                peer: "p".into(),
                channel: Some(0)
            }
        );
    }

    #[test]
    fn parses_branch_ports() {
        assert_eq!(
            audio("audio:branch2:ch1"),
            AudioPort::Branch {
                branch: 2,
                port: Box::new(AudioPort::Channel(1))
            }
        );
    }

    #[test]
    fn parses_control_ports() {
        assert_eq!(
            "control:power".parse::<Port>(),
            Ok(Port::Control("power".into()))
        );
    }

    #[test]
    fn rejects_malformed_handles() {
        for s in [
            "ch1",
            "main",
            "audio:",
            "audio:ch",
            "audio:ch0",
            "audio:st0",
            "audio:mix",
            "audio:peer:",
            "audio:peer:p:x",
            "audio:branch0:ch1",
            "audio:branch1",
            "audio:branch1:branch2:ch1",
            "control:",
            "video:main",
        ] {
            assert!(s.parse::<Port>().is_err(), "{s} should be rejected");
        }
    }

    #[test]
    fn channel_geometry() {
        assert_eq!(AudioPort::Channel(3).tap_width(), Some(1));
        assert_eq!(AudioPort::Stereo(2).tap_width(), Some(2));
        assert_eq!(AudioPort::Main.tap_width(), None);
        assert_eq!(AudioPort::Channel(4).route(), Some((3, 1)));
        assert_eq!(AudioPort::Stereo(2).route(), Some((1, 2)));
        assert_eq!(AudioPort::Sidechain.route(), None);
        assert_eq!(AudioPort::Channel(4).channel_span(), Some(4));
        assert_eq!(AudioPort::Stereo(2).channel_span(), Some(3));
    }

    #[test]
    fn deserializes_from_json_strings() {
        let p: Port = serde_json::from_str("\"audio:ch2\"").unwrap();
        assert_eq!(p, Port::Audio(AudioPort::Channel(2)));
        assert!(serde_json::from_str::<Port>("null").is_err());
        assert!(serde_json::from_str::<Port>("\"ch2\"").is_err());
    }
}
