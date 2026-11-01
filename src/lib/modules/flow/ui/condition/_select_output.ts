import { useSvelteFlow } from '@xyflow/svelte';
import { methods as audioMethods } from '$lib/modules/audio/methods';

/** Opens output `k` of an If or Switch, in the editor and in the engine. */
export function useSelectOutput(id: string): (k: number) => void {
	const flow = useSvelteFlow();
	return (k) => {
		const current = flow.getNode(id)?.data as { selected: number } | undefined;
		if (!current || current.selected === k) return;
		flow.updateNodeData(id, { selected: k });
		audioMethods.updateEffect(id, { selected: k }).catch(() => {});
	};
}
