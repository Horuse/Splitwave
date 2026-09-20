import { describe, expect, it } from 'bun:test';
import { reconcilePendingChange } from '../src/lib/modules/audio/reconnect';

describe('pending audio reconnect', () => {
	it('keeps the old pending state after a transient error so the change is retried', async () => {
		const current = new Set(['missing-input']);
		const resolved = new Set<string>();
		let attempts = 0;
		const reconcile = async () => {
			attempts += 1;
			if (attempts === 1) throw new Error('temporary backend failure');
		};

		expect(await reconcilePendingChange(current, resolved, 'graph', reconcile, async () => {})).toBe('failed');
		expect(await reconcilePendingChange(current, resolved, 'graph', reconcile, async () => {})).toBe('applied');
		expect(attempts).toBe(2);
	});

	it('falls back to starting the graph only when reconciliation reports not running', async () => {
		const calls: string[] = [];
		const result = await reconcilePendingChange(
			new Set(['missing-input']),
			new Set(),
			'graph',
			async () => {
				calls.push('reconcile');
				throw new Error('pipeline not running');
			},
			async () => {
				calls.push('start');
			}
		);

		expect(result).toBe('applied');
		expect(calls).toEqual(['reconcile', 'start']);
	});

	it('does not touch the backend while the unresolved node set is unchanged', async () => {
		let calls = 0;
		const result = await reconcilePendingChange(
			new Set(['missing-input']),
			new Set(['missing-input']),
			'graph',
			async () => {
				calls += 1;
			},
			async () => {
				calls += 1;
			}
		);

		expect(result).toBe('unchanged');
		expect(calls).toBe(0);
	});
});
