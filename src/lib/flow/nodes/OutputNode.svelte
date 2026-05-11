<script lang="ts">
	import { Handle, Position, useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { OutputNodeData } from '$lib/domain/audio-node';
	import { useAppStore } from '$lib/stores/app-store.svelte';

	type OutputNodeType = Node<OutputNodeData, 'output'>;
	let { id, data }: NodeProps<OutputNodeType> = $props();

	const store = useAppStore();
	const flow = useSvelteFlow();

	function onChange(e: Event) {
		const value = (e.currentTarget as HTMLSelectElement).value || null;
		flow.updateNodeData(id, { deviceId: value });
	}
</script>

<div class="min-w-[200px] rounded-md border border-gray-300 bg-white p-3 shadow-sm">
	<div class="mb-2 text-xs font-semibold tracking-wide text-gray-500 uppercase">Output</div>
	<select
		class="w-full rounded border px-2 py-1 text-sm"
		value={data.deviceId ?? ''}
		onchange={onChange}
	>
		<option value="">— Select speaker —</option>
		{#each store.outputDevices as device (device.id)}
			<option value={device.id}>{device.name}</option>
		{/each}
	</select>
	<Handle type="target" position={Position.Left} />
</div>
