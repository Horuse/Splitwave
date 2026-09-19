import { describe, expect, it } from 'bun:test';
import {
	formatBytes,
	formatDb,
	formatDuration,
	formatFreq,
	formatGain,
	formatHz,
	formatKhzValue,
	formatPct,
	formatRate,
	LossWindow
} from '../src/lib/components/format';

describe('formatRate', () => {
	it('formats bytes and units', () => {
		expect(formatRate(0)).toBe('0 B/s');
		expect(formatRate(0.5)).toBe('0 B/s');
		expect(formatRate(512)).toBe('512 B/s');
		expect(formatRate(1024)).toBe('1.0 kB/s');
		expect(formatRate(12_300)).toBe('12.0 kB/s');
		expect(formatRate(1024 * 1024)).toBe('1.00 MB/s');
		expect(formatRate(1.4 * 1024 * 1024)).toBe('1.40 MB/s');
	});
});

describe('formatKhzValue / formatHz / formatFreq', () => {
	it('renders kHz representations', () => {
		expect(formatKhzValue(48_000)).toBe('48');
		expect(formatKhzValue(44_100)).toBe('44.1');
		expect(formatKhzValue(48_002)).toBe('48.002');
		expect(formatHz(48_000)).toBe('48 kHz');
		expect(formatHz(44_100)).toBe('44.1 kHz');
	});

	it('stays in Hz below 1 kHz', () => {
		expect(formatHz(440)).toBe('440 Hz');
		expect(formatFreq(500)).toBe('500');
		expect(formatFreq(1_000)).toBe('1k');
		expect(formatFreq(2_500)).toBe('2.5k');
		expect(formatFreq(16_000)).toBe('16k');
	});
});

describe('formatPct', () => {
	it('formats whole and decimal percentages', () => {
		expect(formatPct(75.4)).toBe('75%');
		expect(formatPct(12.5, 1)).toBe('12.5%');
		expect(formatPct(Number.NaN)).toBe('0%');
		expect(formatPct(Infinity)).toBe('0%');
	});
});

describe('formatBytes', () => {
	it('formats byte sizes with unit ladder', () => {
		expect(formatBytes(0)).toBe('0 B');
		expect(formatBytes(-5)).toBe('0 B');
		expect(formatBytes(Number.NaN)).toBe('0 B');
		expect(formatBytes(512)).toBe('512 B');
		expect(formatBytes(1024)).toBe('1.0 KB');
		expect(formatBytes(24.5 * 1024)).toBe('24.5 KB');
		expect(formatBytes(120.4 * 1024 * 1024)).toBe('120.4 MB');
		expect(formatBytes(1.25 * 1024 * 1024 * 1024)).toBe('1.25 GB');
	});
});

describe('formatDuration', () => {
	it('formats clock times', () => {
		expect(formatDuration(0)).toBe('0:00');
		expect(formatDuration(-5)).toBe('0:00');
		expect(formatDuration(Number.NaN)).toBe('0:00');
		expect(formatDuration(45)).toBe('0:45');
		expect(formatDuration(65.4, 1)).toBe('1:05.4');
		expect(formatDuration(3932)).toBe('1:05:32');
		expect(formatDuration(3661, 2)).toBe('1:01:01.00');
	});
});

describe('formatGain / formatDb', () => {
	it('signs gains and floors dB', () => {
		expect(formatGain(3)).toBe('+3.0');
		expect(formatGain(-6)).toBe('-6.0');
		expect(formatGain(0)).toBe('0.0');
		expect(formatDb(-12.4)).toBe('-12.4');
		expect(formatDb(-120)).toBe('−∞');
		expect(formatDb(Number.NaN)).toBe('−∞');
		// The floor is inclusive: at or below floor → −∞, above → number.
		expect(formatDb(-96)).toBe('−∞');
		expect(formatDb(-90.5)).toBe('-90.5');
		expect(formatDb(-90.5, -90)).toBe('−∞');
	});
});

describe('LossWindow', () => {
	it('starts at 0 and smooths an instantaneous loss ratio', () => {
		const w = new LossWindow(1); // alpha=1: no smoothing, easier assertions
		expect(w.update(100, 10)).toBeCloseTo(0.0909, 3); // 10 lost / 110 total
	});

	it('EMA-smooths across samples with the default alpha', () => {
		const w = new LossWindow();
		// First sample: EMA starts at the instantaneous ratio.
		expect(w.update(90, 10)).toBeCloseTo(0.1, 3);
		// Next sample is clean; the EMA relaxes toward 0 by (1-alpha).
		const next = w.update(190, 10);
		expect(next).toBeCloseTo(0.3 * 0 + 0.7 * 0.1, 3);
	});

	it('returns the previous EMA when no packets arrived', () => {
		const w = new LossWindow();
		const seeded = w.update(90, 10);
		// Same counters again → zero delta → EMA holds.
		expect(w.update(90, 10)).toBeCloseTo(seeded);
	});

	it('restarts on counter reset (reconnect)', () => {
		const w = new LossWindow(1);
		w.update(90, 10);
		// Counters went backwards → start over, report 0.
		expect(w.update(5, 0)).toBe(0);
		// The next delta measures only from the restart point.
		const after = w.update(15, 5);
		expect(after).toBeCloseTo(5 / 15, 3);
	});

	it('reset() clears everything', () => {
		const w = new LossWindow();
		w.update(90, 10);
		w.reset();
		expect(w.update(100, 50)).toBeCloseTo(50 / 150, 3);
	});

	it('never exceeds 1 or goes below 0', () => {
		const w = new LossWindow(1);
		expect(w.update(0, 100)).toBeCloseTo(1);
		expect(w.update(50, 0)).toBeCloseTo(0);
	});
});
