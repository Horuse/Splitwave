//! UDP audio sender. One instance per NetSender node (keyed by node id) owns a
//! send socket and a background task that drains per-channel send rings, encodes
//! each channel (Opus or raw PCM) and transmits it to the configured target as
//! self-describing packets. The DAG runs a NetSender output at 48 kHz, so the
//! task encodes the drained samples directly with no resample.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use rtrb::Consumer;
use tokio::net::UdpSocket;
use tracing::{info, warn};

use crate::audio::graph::OpusApplication;

use super::codec::ChannelEncoder;
use super::packet::{self, Format};

/// Immutable config; a change (target, codec, bitrate, application) rebuilds the
/// sender so the encoder and socket are recreated cleanly.
#[derive(Clone, PartialEq, Eq)]
struct Config {
    target: SocketAddr,
    format: Format,
    opus_bitrate: u32,
    opus_application: OpusApplication,
}

pub struct NetSender {
    config: Config,
    send_consumers: Arc<Mutex<Vec<Consumer<f32>>>>,
    task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
}

static REGISTRY: OnceLock<Mutex<HashMap<String, Arc<NetSender>>>> = OnceLock::new();

fn registry() -> &'static Mutex<HashMap<String, Arc<NetSender>>> {
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Returns the sender for `node_id`, binding the send socket on first use. A
/// config change (target / codec / bitrate) tears the old task down and rebuilds.
pub fn get_or_create(
    node_id: &str,
    target: SocketAddr,
    format: Format,
    opus_bitrate: u32,
    opus_application: OpusApplication,
) -> Arc<NetSender> {
    let config = Config { target, format, opus_bitrate, opus_application };
    let mut reg = registry().lock().unwrap();
    if let Some(s) = reg.get(node_id) {
        if s.config == config {
            return s.clone();
        }
        s.stop();
        reg.remove(node_id);
    }
    let sender = Arc::new(NetSender {
        config,
        send_consumers: Arc::new(Mutex::new(Vec::new())),
        task: Mutex::new(None),
    });
    sender.clone().spawn_send();
    reg.insert(node_id.to_string(), sender.clone());
    sender
}

impl NetSender {
    /// Replace the per-channel send rings the task drains. Called on every
    /// (re)build of this node's output sub-graph.
    pub fn set_send_consumers(&self, consumers: Vec<Consumer<f32>>) {
        *self.send_consumers.lock().unwrap() = consumers;
    }

    fn stop(&self) {
        if let Some(t) = self.task.lock().unwrap().take() {
            t.abort();
        }
    }

    fn spawn_send(self: Arc<Self>) {
        let handle = tauri::async_runtime::spawn(self.clone().send_loop());
        *self.task.lock().unwrap() = Some(handle);
    }

    async fn send_loop(self: Arc<Self>) {
        let socket = match UdpSocket::bind(("0.0.0.0", 0)).await {
            Ok(s) => s,
            Err(e) => {
                warn!(error = %e, "net sender bind failed");
                return;
            }
        };
        let target = self.config.target;
        let application = match self.config.opus_application {
            OpusApplication::Voip => opus::Application::Voip,
            OpusApplication::Audio => opus::Application::Audio,
            OpusApplication::LowDelay => opus::Application::LowDelay,
        };
        info!(%target, "net sender started");

        let consumers = self.send_consumers.clone();
        let format = self.config.format;
        let bitrate = self.config.opus_bitrate;

        let mut encoders: Vec<ChannelEncoder> = Vec::new();
        let mut ins: Vec<Vec<f32>> = Vec::new();
        let mut seqs: Vec<u16> = Vec::new();
        let mut interval = tokio::time::interval(Duration::from_millis(20));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            interval.tick().await;

            // Drain each channel's send ring under the lock, then release it
            // before the encode / send work.
            {
                let mut cons = consumers.lock().unwrap();
                let n = cons.len();
                while encoders.len() < n {
                    encoders.push(ChannelEncoder::new(format, bitrate, application));
                    ins.push(Vec::new());
                    seqs.push(0);
                }
                encoders.truncate(n);
                ins.truncate(n);
                seqs.truncate(n);
                for (i, c) in cons.iter_mut().enumerate() {
                    ins[i].clear();
                    let take = c.slots();
                    if take > 0 {
                        if let Ok(chunk) = c.read_chunk(take) {
                            let (a, b) = chunk.as_slices();
                            ins[i].extend_from_slice(a);
                            ins[i].extend_from_slice(b);
                            chunk.commit_all();
                        }
                    }
                }
            }

            let mut packets: Vec<Vec<u8>> = Vec::new();
            for i in 0..encoders.len() {
                let channel = i as u8;
                let seq = &mut seqs[i];
                encoders[i].push(&ins[i], |payload| {
                    let mut d = Vec::with_capacity(packet::HEADER_LEN + payload.len());
                    packet::write_header(&mut d, format, channel, *seq);
                    *seq = seq.wrapping_add(1);
                    d.extend_from_slice(payload);
                    packets.push(d);
                });
            }
            for p in &packets {
                if let Err(e) = socket.send_to(p, target).await {
                    warn!(%target, error = %e, "net sender send failed");
                }
            }
        }
    }
}
