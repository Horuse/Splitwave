use rtrb::Producer;

use super::Effect;
use crate::audio::streams::bulk_push;
use crate::audio::webrtc_node::PeerSnapshotMap;

pub struct WebRtcBridgeEffect {
    pub send_producer: Producer<f32>,
    pub peer_snapshots: PeerSnapshotMap,
}

impl Effect for WebRtcBridgeEffect {
    fn process(&mut self, samples: &mut [f32], _frames: usize) {
        bulk_push(&mut self.send_producer, samples);

        // The default "mixed" output is the sum of every connected peer.
        samples.fill(0.0);
        if let Ok(snapshots) = self.peer_snapshots.try_lock() {
            for snap in snapshots.values() {
                if let Ok(snap) = snap.try_lock() {
                    let n = snap.len().min(samples.len());
                    for (dst, &v) in samples[..n].iter_mut().zip(snap[..n].iter()) {
                        *dst += v;
                    }
                }
            }
        }
    }

    fn latency_frames(&self) -> usize {
        0
    }
}

impl WebRtcBridgeEffect {
    pub fn populate_handle_bufs(&self, handle_bufs: &mut [(String, Vec<f32>)], _frames: usize) {
        if handle_bufs.is_empty() {
            return;
        }
        let Ok(snapshots) = self.peer_snapshots.try_lock() else {
            for (_, buf) in handle_bufs.iter_mut() {
                buf.fill(0.0);
            }
            return;
        };
        for (handle_id, buf) in handle_bufs.iter_mut() {
            buf.fill(0.0);
            let Some(peer_id) = handle_id.strip_prefix("peer:") else {
                continue;
            };
            if let Some(snap) = snapshots.get(peer_id) {
                if let Ok(snap) = snap.try_lock() {
                    let n = snap.len().min(buf.len());
                    buf[..n].copy_from_slice(&snap[..n]);
                }
            }
        }
    }
}
