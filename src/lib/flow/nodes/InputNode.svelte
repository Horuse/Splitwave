<script lang="ts">
	import { Handle, Position, useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { InputNodeData } from '$lib/domain/audio-node';
	import { useAppStore } from '$lib/stores/app-store.svelte';

	type InputNodeType = Node<InputNodeData, 'input'>;
	let { id, data }: NodeProps<InputNodeType> = $props();

	const store = useAppStore();
	const flow = useSvelteFlow();

	function onChange(e: Event) {
		const value = (e.currentTarget as HTMLSelectElement).value || null;
		flow.updateNodeData(id, { deviceId: value });
	}
</script>

<div class="size-32 border-0 rounded-md bg-neutral-300 p-3">
	<div class="mb-2 text-xs font-semibold tracking-wide text-gray-500 uppercase">Input</div>
	<select
		class="w-full input-base rounded border px-2 py-1 text-sm"
		value={data.deviceId ?? ''}
		onchange={onChange}
	>
		<option value="">— Select microphone —</option>
		{#each store.inputDevices as device (device.id)}
			<option value={device.id}>{device.name}</option>
		{/each}
	</select>
	<Handle type="source" position={Position.Right} />
</div>
