import { getEnginePort } from '../../engine';
import { useSvelteFlow } from '@xyflow/svelte';

/** Opens output `k` of an If or Switch, in the editor and in the engine. */
export function useSelectOutput(id: string): (k: number) => void {
	const flow = useSvelteFlow();
	const engine = getEnginePort();
	return (k) => {
		const current = flow.getNode(id)?.data as { selected: number } | undefined;
		if (!current || current.selected === k) return;
		flow.updateNodeData(id, { selected: k });
		engine.audio.updateEffect(id, { selected: k }).catch(() => {});
	};
}
