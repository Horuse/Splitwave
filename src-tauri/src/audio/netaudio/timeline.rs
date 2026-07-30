//! Per-channel packet timeline.
//!
//! Every channel of a source is encoded from one tick and carries a fixed
//! sample count per packet, so a packet's `seq` *is* its position on the
//! source's timeline. Turning each arrival into a fixed number of samples --
//! filling losses, refusing duplicates and reordered packets -- keeps every
//! channel of the source at the same position no matter how the network
//! delivered them. Aligning on arrival instead leaves a lost packet, or a late
//! one, offsetting that channel against its siblings for good.

/// Losses beyond this are an outage, not a gap: filling them would inject
/// seconds of concealment, so the jitter buffer re-primes the source instead.
const MAX_GAP_PACKETS: u16 = 50;
/// Consecutive rejected packets that mean the sender restarted its counter
/// rather than the network reordering a few packets.
const RESTART_RUN: u32 = 25;

pub enum SeqStep {
    /// Continues the timeline, with `gap` lost packets to conceal before it.
    Advance { gap: u16 },
    /// Timeline broke (long outage, or a sender that restarted its counter).
    /// Decode the packet but conceal nothing -- the buffer re-primes.
    Resync,
    /// Duplicate or reordered: its samples are already on the timeline.
    Drop,
}

#[derive(Default)]
pub struct ChannelTimeline {
    last_seq: Option<u16>,
    rejected_run: u32,
}

impl ChannelTimeline {
    pub fn step(&mut self, seq: u16) -> SeqStep {
        let Some(last) = self.last_seq else {
            self.last_seq = Some(seq);
            return SeqStep::Advance { gap: 0 };
        };
        let delta = seq.wrapping_sub(last);
        // Forward half of the wrapping range; anything else arrived late.
        if delta != 0 && delta < u16::MAX / 2 {
            self.last_seq = Some(seq);
            self.rejected_run = 0;
            let gap = delta - 1;
            return if gap > MAX_GAP_PACKETS {
                SeqStep::Resync
            } else {
                SeqStep::Advance { gap }
            };
        }
        self.rejected_run += 1;
        if self.rejected_run >= RESTART_RUN {
            self.last_seq = Some(seq);
            self.rejected_run = 0;
            return SeqStep::Resync;
        }
        SeqStep::Drop
    }
}
