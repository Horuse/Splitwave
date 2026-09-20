import { describe, expect, it } from 'bun:test';
import { relativeTime } from '../src/lib/utils/time';
import { pruneDanglingEdges } from '../src/lib/modules/pipeline/sanitize';
import { PIPELINE_VERSION, versionOf } from '../src/lib/modules/pipeline/version';
import type { Pipeline } from '../src/lib/modules/pipeline/types';

describe('relativeTime', () => {
	const now = Date.now();

	it('buckets seconds, minutes, hours', () => {
		expect(relativeTime(now - 5_000)).toBe('5s ago');
		expect(relativeTime(now - 120_000)).toBe('2m ago');
		expect(relativeTime(now - 7_200_000)).toBe('2h ago');
	});

	it('switches to a calendar label past a day', () => {
		const out = relativeTime(now - 90_000_000);
		// "M/D HH:MM" — check the shape, not the exact local time.
		expect(/^\d{1,2}\/\d{1,2} \d{2}:\d{2}$/.test(out)).toBe(true);
	});

	it('never goes negative for future timestamps', () => {
		const out = relativeTime(now + 10_000);
		expect(out.startsWith('-')).toBe(false);
		expect(out).toBe('0s ago');
	});
});

function pipelineWith(edges: Array<[string, string]>) {
	return {
		id: 'p1',
		name: 'test',
		createdAt: 0,
		updatedAt: 0,
		nodes: [
			{ id: 'a', kind: 'microphone', data: {}, position: { x: 0, y: 0 } },
			{ id: 'b', kind: 'speaker', data: {}, position: { x: 1, y: 1 } }
		] as any,
		edges: edges.map(([source, target], i) => ({
			id: `e${i}`,
			source,
			target
		})) as any
	};
}

describe('pruneDanglingEdges', () => {
	it('drops edges whose endpoint is gone', () => {
		const p = pipelineWith([
			['a', 'b'],
			['b', 'ghost'],
			['phantom', 'a']
		]);
		const pruned = pruneDanglingEdges(p);
		expect(pruned.edges.length).toBe(1);
		expect(pruned.edges[0]).toEqual({ id: 'e0', source: 'a', target: 'b' });
	});

	it('returns the same object when nothing is dangling', () => {
		const p = pipelineWith([['a', 'b']]);
		expect(pruneDanglingEdges(p)).toBe(p);
	});

	it('handles an empty graph', () => {
		const p = pipelineWith([]);
		expect(pruneDanglingEdges(p)).toBe(p);
	});
});

describe('PIPELINE_VERSION', () => {
	it('versionOf defaults to 0 for unversioned pipelines', () => {
		expect(versionOf({ id: 'x', name: 'x', nodes: [], edges: [], createdAt: 0, updatedAt: 0 })).toBe(0);
		expect(versionOf({ id: 'x', name: 'x', nodes: [], edges: [], createdAt: 0, updatedAt: 0, version: 2 })).toBe(2);
		expect(PIPELINE_VERSION).toBe(2);
	});
});
