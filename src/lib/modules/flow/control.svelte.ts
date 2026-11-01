import { SvelteMap } from 'svelte/reactivity';
import { useNodeConnections } from '@xyflow/svelte';

// The control layer runs here, not in the engine: a source writes its output,
// every input reads the cables wired into it, and Svelte carries the change.
const outputs = new SvelteMap<string, boolean>();

function key(nodeId: string, handle: string): string {
	return `${nodeId}|${handle}`;
}

export const controlSignals = {
	emit(nodeId: string, handle: string, value: boolean) {
		outputs.set(key(nodeId, handle), value);
	},
	clear(nodeId: string, handle: string) {
		outputs.delete(key(nodeId, handle));
	},
	read(nodeId: string, handle: string): boolean {
		return outputs.get(key(nodeId, handle)) ?? false;
	}
};

/** Any cable wired into `handle` that is high; null while nothing is wired. */
export function useControlInput(nodeId: string, handle: string): { readonly current: boolean | null } {
	const cables = useNodeConnections({ id: nodeId, handleType: 'target', handleId: handle });
	return {
		get current() {
			const wired = cables.current;
			if (wired.length === 0) return null;
			return wired.some((c) => !!c.sourceHandle && controlSignals.read(c.source, c.sourceHandle));
		}
	};
}
