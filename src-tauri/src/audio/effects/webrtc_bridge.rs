use rtrb::Producer;

use super::Effect;
use crate::audio::stream_recv::ChannelReceiver;
use crate::audio::streams::bulk_push;

pub struct WebRtcBridgeEffect {
    // One send ring per local channel, indexed by channel number.
    pub send_producers: Vec<Producer<f32>>,
    pub receiver: ChannelReceiver,
}

impl Effect for WebRtcBridgeEffect {
    /// Fills `samples` with the global mix (every peer, every channel); this
    /// pops each tap once, and `populate_handle_bufs` then reads the popped
    /// scratch for the per-peer/per-channel outputs.
    fn process(&mut self, samples: &mut [f32], _frames: usize) {
        self.receiver.mix_block(samples);
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

    /// Handle ids: `peer:<id>:<ch>` = one channel of a peer (tap keyed
    /// `<id>:<ch>`), `peer:<id>` = that peer's channel mix. Reads the tap
    /// `scratch` filled by `process`.
    pub fn populate_handle_bufs(&self, handle_bufs: &mut [(String, Vec<f32>)], _frames: usize) {
        for (handle_id, buf) in handle_bufs.iter_mut() {
            let Some(rest) = handle_id.strip_prefix("peer:") else {
                buf.fill(0.0);
                continue;
            };
            if rest.contains(':') {
                // Specific channel: `rest` is the tap key directly.
                self.receiver.channel(rest, buf);
            } else {
                // Peer mix: sum every channel of this peer.
                self.receiver.prefix_mix(&format!("{rest}:"), buf);
            }
        }
    }
}
