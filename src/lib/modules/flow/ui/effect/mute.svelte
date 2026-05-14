<script lang="ts">
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { MuteNodeData } from '$lib/modules/pipeline/types';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import Wrapper from '../node.svelte';

	type MuteNodeType = Node<MuteNodeData, 'mute'>;
	let { id, data }: NodeProps<MuteNodeType> = $props();

	const flow = useSvelteFlow();

	function toggle() {
		const patch = { muted: !data.muted };
		flow.updateNodeData(id, patch);
		audioMethods.updateEffect(id, patch).catch(() => {});
	}

	function toggleBypass() {
		const patch = { bypassed: !data.bypassed };
		flow.updateNodeData(id, patch);
		audioMethods.updateEffect(id, patch).catch(() => {});
	}
</script>

<Wrapper
	label="Mute"
	accent="effect"
	hasInput
	hasOutput
	bypassed={data.bypassed}
	onBypass={toggleBypass}
>
	<button
		title="Toggle mute (M)"
		class={[
			'nodrag nopan flex w-40 items-center justify-center gap-2 rounded-lg border px-3 py-2 transition-colors',
			data.muted
				? 'border-red-500/60 bg-red-500/10'
				: 'border-neutral-400 bg-neutral-100 hover:bg-neutral-200'
		]}
		onclick={toggle}
	>
		<span
			class={[
				'relative flex h-6 w-6 items-center justify-center rounded-full font-mono text-sm font-bold transition-colors',
				data.muted
					? 'bg-red-500 text-white shadow-[0_0_8px_rgba(239,68,68,0.7)]'
					: 'bg-neutral-300 text-neutral-600'
			]}
		>
			M
			{#if data.muted}
				<span class="absolute inset-0 animate-ping rounded-full bg-red-500/40"></span>
			{/if}
		</span>
		<span class={[
			'text-sm font-medium',
			data.muted ? 'text-red-500' : 'text-neutral-1100'
		]}>
			{data.muted ? 'MUTED' : 'Active'}
		</span>
	</button>
</Wrapper>
