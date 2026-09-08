//! UDP audio receiver. One instance per NetReceiver node (keyed by node id)
//! binds a port and demuxes incoming packets by channel index; each channel is
//! decoded to 48 kHz and fanned out to every output subgraph via `FanoutRegistry`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use tokio::net::UdpSocket;
use tracing::{info, warn};

use crate::audio::stream_recv::{ChannelBroadcast, ConsumerHandle, FanoutRegistry};

use super::codec::ChannelDecoder;
use super::packet;
use super::timeline::{ChannelTimeline, SeqStep};

struct ChannelState {
    decoder: Mutex<ChannelDecoder>,
    // Decoded 48 kHz audio is pushed straight into every consumer's ring.
    broadcast: ChannelBroadcast,
    timeline: Mutex<ChannelTimeline>,
}

pub struct NetReceiver {
    port: u16,
    fanout: FanoutRegistry,
    channels: Mutex<HashMap<u8, Arc<ChannelState>>>,
    task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    bytes: AtomicU64,
    packets: AtomicU64,
    lost: AtomicU64,
    sample_rate: AtomicU32,
    format: AtomicU32,
    opus_bitrate: AtomicU32,
    opus_app: AtomicU32,
}

pub struct ReceiverStatsSnapshot {
    pub bytes: u64,
    pub packets: u64,
    pub lost: u64,
    pub channels: u32,
    pub buffer_ms: u32,
    pub sample_rate: u32,
    pub format: Option<packet::Format>,
    pub opus_bitrate: Option<u32>,
    pub opus_app: Option<u8>,
}

/// Cumulative and link stats since this receiver bound its socket.
pub fn stats(node_id: &str) -> Option<ReceiverStatsSnapshot> {
    let reg = registry().lock().unwrap();
    reg.get(node_id).map(|r| {
        let channels = r
            .channels
            .lock()
            .unwrap()
            .keys()
            .max()
            .map(|&i| i as u32 + 1)
            .unwrap_or(0);
        let sample_rate = r.sample_rate.load(Ordering::Relaxed);
        let sr = if sample_rate > 0 {
            sample_rate
        } else {
            super::SR
        };
        let buffer_ms = r
            .fanout
            .buffer_depth()
            .map(|samples| samples * 1000 / sr)
            .unwrap_or(0);
        let fmt_raw = r.format.load(Ordering::Relaxed);
        let format = if fmt_raw <= 2 {
            packet::Format::from_byte(fmt_raw as u8)
        } else {
            None
        };
        let opus_bitrate_raw = r.opus_bitrate.load(Ordering::Relaxed);
        let opus_bitrate = if opus_bitrate_raw > 0 {
            Some(opus_bitrate_raw)
        } else {
            None
        };
        let opus_app_raw = r.opus_app.load(Ordering::Relaxed);
        let opus_app = if opus_app_raw > 0 {
            Some(opus_app_raw as u8)
        } else {
            None
        };
        ReceiverStatsSnapshot {
            bytes: r.bytes.load(Ordering::Relaxed),
            packets: r.packets.load(Ordering::Relaxed),
            lost: r.lost.load(Ordering::Relaxed),
            channels,
            buffer_ms,
            sample_rate,
            format,
            opus_bitrate,
            opus_app,
        }
    })
}

static REGISTRY: OnceLock<Mutex<HashMap<String, Arc<NetReceiver>>>> = OnceLock::new();

fn registry() -> &'static Mutex<HashMap<String, Arc<NetReceiver>>> {
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Drops a receiver and frees its port. Without this the socket outlives the
/// node, since binding is no longer tied to the pipeline's lifetime.
pub fn release(node_id: &str) {
    let mut reg = registry().lock().unwrap();
    if let Some(r) = reg.remove(node_id) {
        r.stop();
    }
}

/// Returns the receiver for `node_id`, binding the socket on first use. A port
/// change tears the old socket down and rebinds.
pub fn get_or_create(node_id: &str, port: u16) -> Arc<NetReceiver> {
    let mut reg = registry().lock().unwrap();
    if let Some(r) = reg.get(node_id) {
        if r.port == port {
            return r.clone();
        }
        r.stop();
        reg.remove(node_id);
    }
    let receiver = Arc::new(NetReceiver {
        port,
        fanout: FanoutRegistry::default(),
        channels: Mutex::new(HashMap::new()),
        task: Mutex::new(None),
        bytes: AtomicU64::new(0),
        packets: AtomicU64::new(0),
        lost: AtomicU64::new(0),
        sample_rate: AtomicU32::new(48_000),
        format: AtomicU32::new(u32::MAX),
        opus_bitrate: AtomicU32::new(0),
        opus_app: AtomicU32::new(0),
    });
    receiver.clone().spawn_recv();
    reg.insert(node_id.to_string(), receiver.clone());
    receiver
}

impl NetReceiver {
    /// New output subgraph consumer at `output_sr`; wired to every live channel.
    pub fn register_consumer(&self, output_sr: u32, realtime: bool) -> ConsumerHandle {
        self.fanout.register_consumer(output_sr, realtime)
    }

    fn stop(&self) {
        if let Some(t) = self.task.lock().unwrap().take() {
            t.abort();
        }
        self.fanout.clear();
    }

    fn spawn_recv(self: Arc<Self>) {
        let handle = tauri::async_runtime::spawn(self.clone().recv_loop());
        *self.task.lock().unwrap() = Some(handle);
    }

    async fn recv_loop(self: Arc<Self>) {
        let socket = match UdpSocket::bind(("0.0.0.0", self.port)).await {
            Ok(s) => s,
            Err(e) => {
                warn!(port = self.port, error = %e, "net receiver bind failed");
                return;
            }
        };
        info!(port = self.port, "net receiver listening");
        let mut buf = vec![0u8; 2048];
        let mut pcm: Vec<f32> = Vec::new();
        loop {
            let n = match socket.recv_from(&mut buf).await {
                Ok((n, _)) => n,
                Err(_) => continue,
            };
            let Some(pkt) = packet::parse(&buf[..n]) else {
                continue;
            };
            self.bytes.fetch_add(n as u64, Ordering::Relaxed);
            self.packets.fetch_add(1, Ordering::Relaxed);
            self.sample_rate.store(pkt.sample_rate, Ordering::Relaxed);
            self.format
                .store(pkt.format.to_byte() as u32, Ordering::Relaxed);
            if let Some(kbps) = pkt.opus_bitrate_kbps {
                self.opus_bitrate
                    .store(kbps as u32 * 1000, Ordering::Relaxed);
            }
            if let Some(app) = pkt.opus_app {
                self.opus_app.store(app as u32, Ordering::Relaxed);
            }
            let channel = self.channel(pkt.channel, pkt.seq);
            let step = channel.timeline.lock().unwrap().step(pkt.seq);
            match step {
                SeqStep::Drop => continue,
                // The break is longer than concealment covers, so this channel
                // no longer sits where its siblings do. Forget it and let the
                // next packet re-attach it in phase.
                SeqStep::Resync => {
                    self.drop_channel(pkt.channel);
                    continue;
                }
                SeqStep::Advance { .. } => {}
            }
            let mut packets = 1u16;
            // Concealment and payload go out as one push, so the channel
            // advances by whole packets on the source's timeline.
            pcm.clear();
            {
                let mut decoder = channel.decoder.lock().unwrap();
                if let SeqStep::Advance { gap } = step {
                    if gap > 0 {
                        self.lost.fetch_add(gap as u64, Ordering::Relaxed);
                        decoder.conceal_packets(pkt.format, gap, &mut pcm);
                        packets += gap;
                    }
                }
                decoder.decode(pkt.format, pkt.payload, &mut pcm);
            }
            if !pcm.is_empty() {
                crate::audio::stream_recv::broadcast_push_sr(
                    &channel.broadcast,
                    pkt.seq,
                    packets,
                    &pcm,
                    pkt.sample_rate,
                );
            }
        }
    }

    fn drop_channel(&self, index: u8) {
        self.channels.lock().unwrap().remove(&index);
        self.fanout.drop_channel(&index.to_string());
    }

    /// Receive state for a channel index, created (and wired to consumers) on
    /// its first packet.
    fn channel(&self, index: u8, first_seq: u16) -> Arc<ChannelState> {
        let mut channels = self.channels.lock().unwrap();
        if let Some(c) = channels.get(&index) {
            return c.clone();
        }
        let broadcast = self.fanout.attach_channel(index.to_string(), first_seq);
        let state = Arc::new(ChannelState {
            decoder: Mutex::new(ChannelDecoder::new()),
            broadcast,
            timeline: Mutex::new(ChannelTimeline::default()),
        });
        channels.insert(index, state.clone());
        state
    }
}
