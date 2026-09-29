//! UDP audio sender. One instance per NetSender node (keyed by node id) owns a
//! send socket and a thread that drains per-channel send rings, encodes each
//! channel (Opus or raw PCM) and transmits it to the configured target as
//! self-describing packets. The DAG rings the thread every block it leaves in
//! the rings, so audio goes out as soon as it exists rather than on a timer.
//! The DAG runs a NetSender output at 48 kHz, so the thread encodes the drained
//! samples directly with no resample.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use rtrb::Consumer;
use tracing::{info, warn};

use crate::audio::graph::OpusApplication;
use crate::audio::wake::Doorbell;

use super::codec::ChannelEncoder;
use super::packet::{self, Format};

/// Longest the send thread sleeps without being rung.
const IDLE_WAKE: Duration = Duration::from_millis(20);

/// Immutable config; a change (target, codec, bitrate, application) rebuilds the
/// sender so the encoder and socket are recreated cleanly.
#[derive(Clone, PartialEq, Eq)]
struct Config {
    target: SocketAddr,
    format: Format,
    opus_bitrate: u32,
    opus_application: OpusApplication,
    sample_rate: u32,
}

pub struct NetSender {
    config: Config,
    send_consumers: Arc<Mutex<Vec<Consumer<f32>>>>,
    /// Bumped whenever the send rings are replaced, so the task rebuilds every
    /// channel's encode state together.
    consumers_gen: Arc<AtomicU64>,
    /// Rung by the DAG after each block it pushes.
    bell: Arc<Doorbell>,
    stopped: Arc<AtomicBool>,
    /// Wakes that came from a ring rather than the idle timeout.
    #[cfg(test)]
    rung: Arc<AtomicU64>,
    bytes: Arc<AtomicU64>,
    packets: Arc<AtomicU64>,
}

/// `(bytes, packets)` sent since this sender bound its socket.
pub fn stats(node_id: &str) -> Option<(u64, u64)> {
    let reg = registry().lock().unwrap();
    reg.get(node_id).map(|s| {
        (
            s.bytes.load(Ordering::Relaxed),
            s.packets.load(Ordering::Relaxed),
        )
    })
}

static REGISTRY: OnceLock<Mutex<HashMap<String, Arc<NetSender>>>> = OnceLock::new();

fn registry() -> &'static Mutex<HashMap<String, Arc<NetSender>>> {
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Drops a sender and frees its socket/task.
pub fn release(node_id: &str) {
    let mut reg = registry().lock().unwrap();
    if let Some(s) = reg.remove(node_id) {
        s.stop();
    }
}

/// Returns the sender for `node_id`, binding the send socket on first use. A
/// config change (target / codec / bitrate / sample rate) tears the old task down and rebuilds.
pub fn get_or_create(
    node_id: &str,
    target: SocketAddr,
    format: Format,
    opus_bitrate: u32,
    opus_application: OpusApplication,
    sample_rate: u32,
) -> Arc<NetSender> {
    let config = Config {
        target,
        format,
        opus_bitrate,
        opus_application,
        sample_rate,
    };
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
        consumers_gen: Arc::new(AtomicU64::new(0)),
        bell: Arc::default(),
        stopped: Arc::new(AtomicBool::new(false)),
        #[cfg(test)]
        rung: Arc::new(AtomicU64::new(0)),
        bytes: Arc::new(AtomicU64::new(0)),
        packets: Arc::new(AtomicU64::new(0)),
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
        self.consumers_gen.fetch_add(1, Ordering::SeqCst);
    }

    /// What the DAG rings once it has pushed a block into the send rings.
    pub fn bell(&self) -> Arc<Doorbell> {
        self.bell.clone()
    }

    fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        self.bell.ring();
    }

    fn spawn_send(self: Arc<Self>) {
        if let Err(e) = std::thread::Builder::new()
            .name("net-send".into())
            .spawn(move || self.send_loop())
        {
            warn!(error = %e, "net sender thread failed to start");
        }
    }

    fn send_loop(&self) {
        self.bell.answer_here();
        let socket = match UdpSocket::bind(("0.0.0.0", 0)) {
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
        let consumers_gen = self.consumers_gen.clone();
        let mut seen_gen = u64::MAX;
        let format = self.config.format;
        let bitrate = self.config.opus_bitrate;

        let mut encoders: Vec<ChannelEncoder> = Vec::new();
        let mut ins: Vec<Vec<f32>> = Vec::new();
        let mut seqs: Vec<u16> = Vec::new();

        while !self.stopped.load(Ordering::SeqCst) {
            // Woken by each block the DAG pushes; the timeout only matters if
            // the DAG stops.
            #[cfg(test)]
            let started = std::time::Instant::now();
            self.bell.wait(IDLE_WAKE);
            #[cfg(test)]
            if started.elapsed() < IDLE_WAKE / 2 {
                self.rung.fetch_add(1, Ordering::Relaxed);
            }

            // Drain each channel's send ring under the lock, then release it
            // before the encode / send work.
            {
                let mut cons = consumers.lock().unwrap();
                // Each encoder holds a partial Opus frame; keeping them across a
                // ring swap would leave a channel added now offset from its
                // siblings by whatever they had buffered.
                let gen = consumers_gen.load(Ordering::SeqCst);
                if gen != seen_gen {
                    seen_gen = gen;
                    encoders.clear();
                    ins.clear();
                    seqs.clear();
                }
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
            let sample_rate = self.config.sample_rate;
            let (opus_bitrate_kbps, opus_app_byte) = if format == Format::Opus {
                let app = match self.config.opus_application {
                    OpusApplication::Voip => 1,
                    OpusApplication::Audio => 2,
                    OpusApplication::LowDelay => 3,
                };
                ((self.config.opus_bitrate / 1000) as u16, app)
            } else {
                (0, 0)
            };
            for i in 0..encoders.len() {
                let channel = i as u8;
                let seq = &mut seqs[i];
                encoders[i].push(&ins[i], |payload| {
                    let mut d = Vec::with_capacity(packet::HEADER_LEN_V2_OPUS + payload.len());
                    packet::write_header(
                        &mut d,
                        format,
                        channel,
                        *seq,
                        sample_rate,
                        opus_bitrate_kbps,
                        opus_app_byte,
                    );
                    *seq = seq.wrapping_add(1);
                    d.extend_from_slice(payload);
                    packets.push(d);
                });
            }
            for p in &packets {
                match socket.send_to(p, target) {
                    Ok(_) => {
                        self.bytes.fetch_add(p.len() as u64, Ordering::Relaxed);
                        self.packets.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(e) => warn!(%target, error = %e, "net sender send failed"),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn sends_numbered_packets_per_channel() {
        let sink = std::net::UdpSocket::bind("127.0.0.1:0").expect("bind sink");
        sink.set_read_timeout(Some(Duration::from_secs(5)))
            .expect("read timeout");
        let target = sink.local_addr().expect("sink address");
        let sender = get_or_create(
            "test-sender",
            target,
            Format::PcmF32,
            0,
            OpusApplication::Audio,
            48_000,
        );

        let mut consumers = Vec::new();
        for value in [0.25f32, -0.5] {
            let (mut prod, cons) = rtrb::RingBuffer::new(9_600);
            prod.write_chunk_uninit(9_600)
                .expect("ring space")
                .fill_from_iter(std::iter::repeat(value));
            consumers.push(cons);
        }
        sender.set_send_consumers(consumers);

        let mut seqs: BTreeMap<u8, Vec<u16>> = BTreeMap::new();
        let mut buf = [0u8; 2048];
        while seqs.values().map(Vec::len).min().unwrap_or(0) < 3 || seqs.len() < 2 {
            let n = sink.recv(&mut buf).expect("sender went quiet");
            let pkt = packet::parse(&buf[..n]).expect("well-formed packet");
            assert_eq!(pkt.format, Format::PcmF32);
            assert_eq!(pkt.sample_rate, 48_000);
            let mut pcm = Vec::new();
            packet::pcm_f32_decode(pkt.payload, &mut pcm);
            let want = if pkt.channel == 0 { 0.25 } else { -0.5 };
            assert!(
                pcm.iter().all(|&s| s == want),
                "channel {} payload",
                pkt.channel
            );
            seqs.entry(pkt.channel).or_default().push(pkt.seq);
        }
        release("test-sender");

        for (channel, run) in seqs {
            for pair in run.windows(2) {
                assert_eq!(pair[1], pair[0].wrapping_add(1), "channel {channel} seq");
            }
        }
        assert!(stats("test-sender").is_none(), "release frees the node");
    }

    #[test]
    fn a_ring_sends_at_once_instead_of_on_the_idle_tick() {
        let sink = std::net::UdpSocket::bind("127.0.0.1:0").expect("bind sink");
        let sender = get_or_create(
            "test-rung-sender",
            sink.local_addr().expect("sink address"),
            Format::PcmF32,
            0,
            OpusApplication::Audio,
            48_000,
        );
        let (mut prod, cons) = rtrb::RingBuffer::new(9_600);
        sender.set_send_consumers(vec![cons]);
        let bell = sender.bell();
        std::thread::sleep(Duration::from_millis(50));
        for _ in 0..5 {
            prod.write_chunk_uninit(64)
                .expect("ring space")
                .fill_from_iter(std::iter::repeat(0.5));
            bell.ring();
            std::thread::sleep(Duration::from_millis(3));
        }
        let rung = sender.rung.load(Ordering::Relaxed);
        release("test-rung-sender");
        assert!(rung >= 3, "only {rung} of 5 rings woke the sender");
    }
}
