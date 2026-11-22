import { BROWSER as browser } from 'esm-env';
import type { LatencyReport } from '@splitwave/nodes/generated/LatencyReport';
import type { NodeTiming } from '@splitwave/nodes/generated/NodeTiming';
import { methods } from './methods';
import { audioStore } from './stores.svelte';

const POLL_MS = 500;

/** Latest engine latency report, polled while a pipeline runs. */
class LatencyStore {
	report = $state<LatencyReport | null>(null);
	#nodes = $derived(new Map<string, NodeTiming>((this.report?.nodes ?? []).map((n) => [n.nodeId, n])));

	node(id: string): NodeTiming | undefined {
		return this.#nodes.get(id);
	}
}

export const latencyStore = new LatencyStore();

if (browser) {
	$effect.root(() => {
		$effect(() => {
			if (!audioStore.isRunning) {
				latencyStore.report = null;
				return;
			}
			let cancelled = false;
			const poll = async () => {
				const report = await methods.getLatencyReport().catch(() => null);
				if (!cancelled) latencyStore.report = report;
			};
			void poll();
			const id = setInterval(() => void poll(), POLL_MS);
			return () => {
				cancelled = true;
				clearInterval(id);
			};
		});
	});
}
