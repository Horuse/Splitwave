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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequential_packets_advance_without_gap() {
        let mut t = ChannelTimeline::default();
        for seq in [0u16, 1, 2, 3] {
            match t.step(seq) {
                SeqStep::Advance { gap } => assert_eq!(gap, 0, "seq {seq}"),
                _ => panic!("sequence {seq} did not advance"),
            }
        }
    }

    #[test]
    fn small_gap_conceals_then_advances() {
        let mut t = ChannelTimeline::default();
        assert!(matches!(t.step(0), SeqStep::Advance { gap: 0 }));
        // Packet 3 arrives after 1 was lost.
        match t.step(3) {
            SeqStep::Advance { gap } => assert_eq!(gap, 2),
            _ => panic!("small gap must advance"),
        }
    }

    #[test]
    fn duplicate_and_reordered_are_dropped() {
        let mut t = ChannelTimeline::default();
        let _ = t.step(5);
        assert!(matches!(t.step(5), SeqStep::Drop), "same seq twice");
        assert!(matches!(t.step(4), SeqStep::Drop), "late arrival");
        assert!(matches!(t.step(3), SeqStep::Drop));
    }

    #[test]
    fn huge_gap_is_a_resync_not_a_fill() {
        let mut t = ChannelTimeline::default();
        let _ = t.step(0);
        match t.step(200) {
            SeqStep::Resync => {}
            _ => panic!("a 200-packet outage must resync"),
        }
    }

    #[test]
    fn restart_run_resyncs() {
        const RESTART_RUN: u32 = 25;
        let mut t = ChannelTimeline::default();
        let _ = t.step(100);
        // A sender restarting at 0: 25 rejected packets in a row flip to resync.
        for seq in [200u16, 201] {
            let _ = t.step(seq);
        }
        let mut rejected = 0;
        let mut last = None;
        for seq in 0..40u16 {
            let step = t.step(seq);
            match step {
                SeqStep::Drop => rejected += 1,
                SeqStep::Resync => {
                    last = Some((rejected, seq));
                    break;
                }
                SeqStep::Advance { .. } => panic!("restart must not advance timeline"),
            }
        }
        let (run, _at) = last.expect("a long run of rejections must flip to resync");
        assert!(run >= RESTART_RUN as u32 - 1, "rejections counted: {run}");
    }
}
