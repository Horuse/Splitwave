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

/** Returns the numeric kHz representation, e.g. 48000 -> "48", 44100 -> "44.1", 48002 -> "48.002". */
export function formatKhzValue(hz: number): string {
	const k = hz / 1000;
	return String(Number(k.toFixed(3)));
}

/** Human-readable audio sample rate / frequency, e.g. `48 kHz`, `44.1 kHz`, `48.002 kHz`, `440 Hz`. */
export function formatHz(hz: number): string {
	if (hz < 1000) return `${hz} Hz`;
	return `${formatKhzValue(hz)} kHz`;
}

/** Compact audio frequency label for ticks and EQ bands, e.g. `32`, `500`, `1k`, `2.5k`, `16k`. */
export function formatFreq(hz: number): string {
	if (hz >= 1000) return `${formatKhzValue(hz)}k`;
	return String(Math.round(hz));
}

/** Percentage representation, e.g. `75%`, `12.5%`. */
export function formatPct(p: number, decimals: number = 0): string {
	if (!Number.isFinite(p)) return '0%';
	if (decimals > 0) return `${p.toFixed(decimals)}%`;
	return `${Math.round(p)}%`;
}

/** Human-readable data size, e.g. `512 B`, `24.5 KB`, `120.4 MB`, `1.25 GB`. */
export function formatBytes(bytes: number): string {
	if (!Number.isFinite(bytes) || bytes <= 0) return '0 B';
	if (bytes < 1024) return `${Math.round(bytes)} B`;
	if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
	if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
	return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

export const formatSize = formatBytes;

/** Time/duration string, e.g. `0:12.4` (decimals=1) or `1:05:32` / `0:45` (decimals=0). */
export function formatDuration(sec: number, decimals: number = 0): string {
	if (!Number.isFinite(sec) || sec <= 0) {
		return decimals > 0 ? `0:00.${'0'.repeat(decimals)}` : '0:00';
	}
	const h = Math.floor(sec / 3600);
	const m = Math.floor((sec % 3600) / 60);
	const s = Math.floor(sec % 60);
	const sStr = String(s).padStart(2, '0');
	const frac = decimals > 0 ? (sec % 1).toFixed(decimals).slice(1) : '';
	if (h > 0) {
		return `${h}:${String(m).padStart(2, '0')}:${sStr}${frac}`;
	}
	return `${m}:${sStr}${frac}`;
}

/** Gain in dB with sign, e.g. `+3.0`, `-6.0`, `0.0`. */
export function formatGain(db: number): string {
	const v = db.toFixed(1);
	return db > 0 ? `+${v}` : v;
}

/** Level meter decibel readout with floor handling, e.g. `-12.4`, `−∞`. */
export function formatDb(db: number, floor: number = -96): string {
	return Number.isFinite(db) && db > floor ? db.toFixed(1) : '−∞';
}
