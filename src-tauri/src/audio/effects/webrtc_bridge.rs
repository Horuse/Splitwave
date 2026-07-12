use rtrb::Producer;

use super::Effect;
use crate::audio::streams::bulk_push;
use crate::audio::webrtc::PeerSnapshotMap;

pub struct WebRtcBridgeEffect {
    // One send ring per local channel, indexed by channel number.
    pub send_producers: Vec<Producer<f32>>,
    pub peer_snapshots: PeerSnapshotMap,
}

impl Effect for WebRtcBridgeEffect {
    /// Fills `samples` with the global mix (every peer, every channel). Each
    /// peer-channel jitter buffer is popped once here into its tap `scratch`,
    /// which `populate_handle_bufs` then reads for the per-peer/per-channel outs.
    fn process(&mut self, samples: &mut [f32], _frames: usize) {
        samples.fill(0.0);
        if let Ok(mut taps) = self.peer_snapshots.try_lock() {
            for tap in taps.values_mut() {
                let n = tap.fill_block(samples.len());
                for (dst, &v) in samples[..n].iter_mut().zip(tap.scratch[..n].iter()) {
                    *dst += v;
                }
            }
        }
    }

    fn latency_frames(&self) -> usize {
        0
    }
}

impl WebRtcBridgeEffect {
    /// Push each channel's mixed input into its send ring; the encode task
    /// tags packets with the channel index matching this order.
    pub fn push_channel_inputs(&mut self, channel_bufs: &[(String, Vec<f32>)]) {
        for (i, (_, buf)) in channel_bufs.iter().enumerate() {
            if let Some(prod) = self.send_producers.get_mut(i) {
                bulk_push(prod, buf);
            }
        }
    }

    /// Handle ids: `peer:<id>:<ch>` = one channel of a peer, `peer:<id>` = that
    /// peer's channel mix. Reads the tap `scratch` filled by `process`.
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
            let Some(rest) = handle_id.strip_prefix("peer:") else {
                continue;
            };
            match rest.rsplit_once(':').and_then(|(p, c)| c.parse::<u8>().ok().map(|c| (p, c))) {
                Some((peer, ch)) => {
                    if let Some(tap) = snapshots.values().find(|t| t.peer == peer && t.channel == ch)
                    {
                        let n = tap.valid.min(buf.len());
                        buf[..n].copy_from_slice(&tap.scratch[..n]);
                    }
                }
                None => {
                    for tap in snapshots.values().filter(|t| t.peer == rest) {
                        let n = tap.valid.min(buf.len());
                        for (dst, &v) in buf[..n].iter_mut().zip(tap.scratch[..n].iter()) {
                            *dst += v;
                        }
                    }
                }
            }
        }
    }
}
