//! Direct-IP (UDP) audio transport. NetReceiver is an Input source and
//! NetSender an Output sink; both carry up to 10 channels at 48 kHz stereo,
//! encoded as Opus or raw PCM (self-describing per packet).
#![allow(dead_code)] // wired into the graph engine by the NetSender/NetReceiver nodes

pub mod codec;
pub mod packet;
pub mod receiver;
pub mod sender;
pub mod timeline;

pub const SR: u32 = 48_000;
/// One 20 ms Opus frame; a net channel is mono, so frames == samples.
pub const OPUS_FRAME_SAMPLES: usize = 960;
/// Channel index rides in one packet header byte (`packet.rs`).
pub const MAX_CHANNELS: usize = 255;
/// Per-channel send ring feeding the UDP task (~2 s mono at 48 kHz).
pub const SEND_RING: usize = 96_000;
