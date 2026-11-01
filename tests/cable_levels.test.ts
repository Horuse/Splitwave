import { describe, expect, it } from 'bun:test';
import { cableLevel, recordLevels } from '../src/lib/modules/flow/levels.svelte';

describe('cableLevel', () => {
	it('reads the channel a cable carries, or the loudest for a bus', () => {
		recordLevels('mic', [1, 0.001]);
		expect(cableLevel('mic', 'audio:ch1')).toBe(1);
		expect(cableLevel('mic', 'audio:ch2')).toBeCloseTo(0.001 ** 0.7, 5);
		expect(cableLevel('mic', 'audio:main')).toBe(1);
	});

	it('maps linear peaks with a gentle lift', () => {
		recordLevels('g', [0.1]);
		expect(cableLevel('g', 'audio:ch1')).toBeCloseTo(0.1 ** 0.7, 5);
	});

	it('meters a branch output on its gate', () => {
		recordLevels('sw#b2', [1, 1]);
		expect(cableLevel('sw', 'audio:branch2:ch1')).toBe(1);
		expect(cableLevel('sw', 'audio:branch1:ch1')).toBe(0);
	});

	it('is silent for unknown sources and control cables', () => {
		expect(cableLevel('nowhere', 'audio:ch1')).toBe(0);
		recordLevels('k', [1]);
		expect(cableLevel('k', 'control:out')).toBe(0);
	});
});
