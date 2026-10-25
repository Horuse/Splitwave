import { describe, expect, it } from 'bun:test';
import { channelColor, channelLabel, darken, deriveSlots, freeRunFrom, handleColor, parseHandle } from '../src/lib/modules/flow/utils/channels';

describe('parseHandle', () => {
	it('reads channel handles and rejects everything else', () => {
		expect(parseHandle('ch1')).toBe(1);
		expect(parseHandle('ch12')).toBe(12);
		expect(parseHandle('sidechain')).toBeNull();
		expect(parseHandle('ch')).toBeNull();
		expect(parseHandle('xch1')).toBeNull();
	});
});

describe('deriveSlots', () => {
	it('keeps freed slots in place instead of renumbering', () => {
		expect(deriveSlots(['ch1', 'ch3'], false)).toEqual([
			{ id: 'ch1', ch: 1, occupied: true },
			{ id: 'ch2', ch: 2, occupied: false },
			{ id: 'ch3', ch: 3, occupied: true }
		]);
	});

	it('appends one trailing free slot', () => {
		expect(deriveSlots(['ch1'], true).map((s) => s.id)).toEqual(['ch1', 'ch2']);
		expect(deriveSlots([], true)).toEqual([{ id: 'ch1', ch: 1, occupied: false }]);
	});

	it('honours min and max', () => {
		expect(deriveSlots([], false, Infinity, 2).map((s) => s.id)).toEqual(['ch1', 'ch2']);
		expect(deriveSlots(['ch1', 'ch2'], true, 2).map((s) => s.id)).toEqual(['ch1', 'ch2']);
		expect(deriveSlots(['ch5'], true, 3).map((s) => s.id)).toEqual(['ch1', 'ch2', 'ch3']);
	});

	it('ignores non-channel handles', () => {
		expect(deriveSlots(['sidechain', 'peer:a:0'], false)).toEqual([]);
	});
});

describe('freeRunFrom', () => {
	it('skips taken channels', () => {
		expect(freeRunFrom(['ch2'], 1, 3)).toEqual([1, 3, 4]);
		expect(freeRunFrom([], 5, 2)).toEqual([5, 6]);
		expect(freeRunFrom(['ch1'], 1, 0)).toEqual([]);
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
		expect(handleColor('ch1')).toBe(channelColor(0));
		expect(handleColor('peer:abc:1')).toBe(channelColor(1));
		expect(handleColor('bus')).toBe(handleColor(null));
		expect(handleColor(undefined)).toBe(handleColor('sidechain'));
	});

	it('darkens hex colours', () => {
		expect(darken('#ffffff', 0.5)).toBe('#808080');
		expect(darken('#000000')).toBe('#000000');
		expect(darken('#0a0b0c', 1)).toBe('#0a0b0c');
	});
});
