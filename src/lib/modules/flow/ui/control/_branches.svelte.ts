import { untrack } from 'svelte';
import type { useSvelteFlow } from '@xyflow/svelte';
import { parsePort, selectHandle } from '$lib/modules/flow/utils';
import { useControlInput } from '../../control.svelte';

export const MIN_BRANCHES = 2;
export const MAX_BRANCHES = 8;

/** One `select k` trigger per branch a Switch or Select can grow to; hooks are
 * fixed at mount. A rising edge on trigger k picks branch k, so a held key
 * never fights a later press. */
export function useBranchTriggers(nodeId: string, live: boolean, count: () => number, pick: (k: number) => void) {
	const triggers = live ? Array.from({ length: MAX_BRANCHES }, (_, i) => useControlInput(nodeId, selectHandle(i + 1))) : [];
	const wasHigh: boolean[] = Array(MAX_BRANCHES).fill(false);
	$effect(() => {
		const n = count();
		triggers.forEach((t, i) => {
			const high = t.current ?? false;
			if (high && !wasHigh[i] && i < n) untrack(() => pick(i + 1));
			wasHigh[i] = high;
		});
	});
	return {
		high(k: number): boolean {
			return triggers[k - 1]?.current ?? false;
		}
	};
}

/** Cables on branches past `count` would point at sockets that no longer exist. */
export function pruneBranches(flow: ReturnType<typeof useSvelteFlow>, nodeId: string, count: number): void {
	const dropped = flow
		.getEdges()
		.filter((e) => {
			const handle = e.source === nodeId ? e.sourceHandle : e.target === nodeId ? e.targetHandle : null;
			const p = parsePort(handle);
			if (p?.signal === 'audio') return p.port.kind === 'branch' && p.port.branch > count;
			return p?.signal === 'control' && Array.from({ length: MAX_BRANCHES - count }, (_, i) => selectHandle(count + i + 1)).includes(handle!);
		})
		.map((e) => ({ id: e.id }));
	if (dropped.length > 0) flow.deleteElements({ edges: dropped });
}
