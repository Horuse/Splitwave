//! UDP audio receiver. One instance per NetReceiver node (keyed by node id)
//! binds a port and demuxes incoming packets by channel index; each channel is
//! decoded to 48 kHz and fanned out to every output subgraph via `FanoutRegistry`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use tokio::net::UdpSocket;
use tracing::{info, warn};

use crate::audio::stream_recv::{broadcast_push, ChannelBroadcast, ConsumerHandle, FanoutRegistry};

use super::codec::ChannelDecoder;
use super::packet;

struct ChannelState {
    decoder: Mutex<ChannelDecoder>,
    // Decoded 48 kHz audio is pushed straight into every consumer's ring.
    broadcast: ChannelBroadcast,
}

pub struct NetReceiver {
    port: u16,
    fanout: FanoutRegistry,
    channels: Mutex<HashMap<u8, Arc<ChannelState>>>,
    task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    bytes: AtomicU64,
    packets: AtomicU64,
    lost: AtomicU64,
}

/// `(bytes, packets, lost)` since this receiver bound its socket.
pub fn stats(node_id: &str) -> Option<(u64, u64, u64)> {
    let reg = registry().lock().unwrap();
    reg.get(node_id).map(|r| {
        (
            r.bytes.load(Ordering::Relaxed),
            r.packets.load(Ordering::Relaxed),
            r.lost.load(Ordering::Relaxed),
        )
    })
}

static REGISTRY: OnceLock<Mutex<HashMap<String, Arc<NetReceiver>>>> = OnceLock::new();

fn registry() -> &'static Mutex<HashMap<String, Arc<NetReceiver>>> {
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
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
        let mut last_seq: HashMap<u8, u16> = HashMap::new();
        loop {
            let n = match socket.recv_from(&mut buf).await {
                Ok((n, _)) => n,
                Err(_) => continue,
            };
            let Some(pkt) = packet::parse(&buf[..n]) else { continue };
            self.bytes.fetch_add(n as u64, Ordering::Relaxed);
            self.packets.fetch_add(1, Ordering::Relaxed);
            let channel = self.channel(pkt.channel);
            let mut gap = 0u16;
            if let Some(prev) = last_seq.insert(pkt.channel, pkt.seq) {
                let g = pkt.seq.wrapping_sub(prev).wrapping_sub(1);
                if g > 0 && (g as u32) < 1000 {
                    self.lost.fetch_add(g as u64, Ordering::Relaxed);
                    gap = g;
                }
            }
            // Opus conceals lost frames from decoder state; PCM has no codec PLC
            // (the playback side fades instead), so only conceal for Opus.
            if gap > 0 && pkt.format == packet::Format::Opus {
                for _ in 0..gap.min(10) {
                    pcm.clear();
                    channel.decoder.lock().unwrap().conceal(&mut pcm);
                    if !pcm.is_empty() {
                        broadcast_push(&channel.broadcast, &pcm);
                    }
                }
            }
            pcm.clear();
            channel.decoder.lock().unwrap().decode(pkt.format, pkt.payload, &mut pcm);
            if !pcm.is_empty() {
                broadcast_push(&channel.broadcast, &pcm);
            }
        }
    }

    /// Receive state for a channel index, created (and wired to consumers) on
    /// its first packet.
    fn channel(&self, index: u8) -> Arc<ChannelState> {
        let mut channels = self.channels.lock().unwrap();
        if let Some(c) = channels.get(&index) {
            return c.clone();
        }
        let broadcast = self.fanout.attach_channel(index.to_string());
        let state = Arc::new(ChannelState {
            decoder: Mutex::new(ChannelDecoder::new()),
            broadcast,
        });
        channels.insert(index, state.clone());
        state
    }
}
