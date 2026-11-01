import { describe, expect, it } from 'bun:test';
import { channelColor, channelLabel, darken, deriveSlots, freeRunFrom, handleColor } from '../src/lib/modules/flow/utils/channels';

describe('deriveSlots', () => {
	it('keeps freed slots in place instead of renumbering', () => {
		expect(deriveSlots(['audio:ch1', 'audio:ch3'], false)).toEqual([
			{ id: 'audio:ch1', ch: 1, occupied: true },
			{ id: 'audio:ch2', ch: 2, occupied: false },
			{ id: 'audio:ch3', ch: 3, occupied: true }
		]);
	});

	it('appends one trailing free slot', () => {
		expect(deriveSlots(['audio:ch1'], true).map((s) => s.id)).toEqual(['audio:ch1', 'audio:ch2']);
		expect(deriveSlots([], true)).toEqual([{ id: 'audio:ch1', ch: 1, occupied: false }]);
	});

	it('honours min and max', () => {
		expect(deriveSlots([], false, Infinity, 2).map((s) => s.id)).toEqual(['audio:ch1', 'audio:ch2']);
		expect(deriveSlots(['audio:ch1', 'audio:ch2'], true, 2).map((s) => s.id)).toEqual(['audio:ch1', 'audio:ch2']);
		expect(deriveSlots(['audio:ch5'], true, 3).map((s) => s.id)).toEqual(['audio:ch1', 'audio:ch2', 'audio:ch3']);
	});

	it('ignores non-channel handles', () => {
		expect(deriveSlots(['audio:sidechain', 'audio:peer:a:0'], false)).toEqual([]);
	});
});

describe('freeRunFrom', () => {
	it('skips taken channels', () => {
		expect(freeRunFrom(['audio:ch2'], 1, 3)).toEqual([1, 3, 4]);
		expect(freeRunFrom([], 5, 2)).toEqual([5, 6]);
		expect(freeRunFrom(['audio:ch1'], 1, 0)).toEqual([]);
	});
});

describe('channel colours and labels', () => {
	it('labels stereo as L/R and wider layouts by number', () => {
		expect(channelLabel(0, 2)).toBe('L');
		expect(channelLabel(1, 2)).toBe('R');
		expect(channelLabel(2, 4)).toBe('ch3');
		expect(channelLabel(0, 1)).toBe('ch1');
	});

	it('cycles the palette', () => {
		expect(channelColor(16)).toBe(channelColor(0));
	});

	it('colours channel and peer handles, neutral otherwise', () => {
		expect(handleColor('audio:ch1')).toBe(channelColor(0));
		expect(handleColor('audio:peer:abc:1')).toBe(channelColor(1));
		expect(handleColor('audio:main')).toBe(handleColor(null));
		expect(handleColor(undefined)).toBe(handleColor('audio:sidechain'));
		expect(handleColor('control:power')).toBe(handleColor(null));
	});

	it('darkens hex colours', () => {
		expect(darken('#ffffff', 0.5)).toBe('#808080');
		expect(darken('#000000')).toBe('#000000');
		expect(darken('#0a0b0c', 1)).toBe('#0a0b0c');
	});
});
