/** Rolling packet-loss estimate from cumulative counters. Loss over the delta
 * since the last sample is EMA-smoothed, so the value reflects the last several
 * seconds rather than the whole session. */
export class LossWindow {
	private prevPackets = 0;
	private prevLost = 0;
	private ema: number | null = null;
	private readonly alpha: number;

	constructor(alpha = 0.3) {
		this.alpha = alpha;
	}

	/** Feed cumulative counters; returns smoothed loss ratio (0..1). */
	update(packets: number, lost: number): number {
		// Counters reset (reconnect / node restart) -> start over.
		if (packets < this.prevPackets || lost < this.prevLost) {
			this.prevPackets = packets;
			this.prevLost = lost;
			this.ema = null;
			return 0;
		}
		const dp = packets - this.prevPackets;
		const dl = lost - this.prevLost;
		this.prevPackets = packets;
		this.prevLost = lost;
		const total = dp + dl;
		if (total <= 0) return this.ema ?? 0;
		const inst = dl / total;
		this.ema = this.ema == null ? inst : this.alpha * inst + (1 - this.alpha) * this.ema;
		return this.ema;
	}

	reset() {
		this.prevPackets = 0;
		this.prevLost = 0;
		this.ema = null;
	}
}

/** Human-readable transfer rate, e.g. `12.3 kB/s`, `1.4 MB/s`. */
export function formatRate(bytesPerSec: number): string {
	if (bytesPerSec < 1) return '0 B/s';
	if (bytesPerSec < 1024) return `${Math.round(bytesPerSec)} B/s`;
	if (bytesPerSec < 1024 * 1024) return `${(bytesPerSec / 1024).toFixed(1)} kB/s`;
	return `${(bytesPerSec / (1024 * 1024)).toFixed(2)} MB/s`;
}
