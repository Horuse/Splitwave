//! Direct-IP (UDP) audio transport. NetReceiver is an Input source and
//! NetSender an Output sink; both carry up to 10 channels at 48 kHz stereo,
//! encoded as Opus or raw PCM (self-describing per packet).
#![allow(dead_code)] // wired into the graph engine by the NetSender/NetReceiver nodes

pub mod codec;
pub mod packet;

pub const SR: u32 = 48_000;
pub const OPUS_FRAME_SAMPLES: usize = 960 * 2;
pub const MAX_CHANNELS: usize = 10;
