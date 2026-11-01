<script lang="ts">
	import { getContext, untrack } from 'svelte';
	import type { Node, NodeProps } from '@xyflow/svelte';
	import type { IfNodeData } from '$lib/modules/pipeline/types';
	import Wrapper from '../node.svelte';
	import ChannelHandles from '../_channel_handles.svelte';
	import { ArrowRight } from '$lib/components/icons';
	import ControlPort from './_control_port.svelte';
	import OutputRow from './_output_row.svelte';
	import { useSelectOutput } from './_select_output';
	import { COND_HANDLE, PREVIEW_CTX } from '$lib/modules/flow/utils';
	import { useControlInput } from '../../control.svelte';

	type IfNodeType = Node<IfNodeData, 'if'>;
	let { id, data }: NodeProps<IfNodeType> = $props();

	const isPreview = getContext(PREVIEW_CTX) === true;
	const nodeId = untrack(() => id);
	const cond = isPreview ? null : useControlInput(nodeId, COND_HANDLE);
	const select = useSelectOutput(nodeId);

	const OUTPUTS = [
		{ branch: 1, label: 'true' },
		{ branch: 2, label: 'false' }
	];

	let wired = $derived(cond?.current != null);

	$effect(() => {
		const high = cond?.current;
		if (high == null) return;
		untrack(() => select(high ? 1 : 2));
	});
</script>

<Wrapper label="If" icon={ArrowRight} accent="condition">
	<div class="flex items-start gap-4">
		<div class="flex flex-col gap-2">
			<ControlPort id={COND_HANDLE} side="target" label="if" high={cond?.current ?? false} />
			{#if !isPreview}
				<ChannelHandles {nodeId} side="target" />
			{/if}
		</div>
		<div class="flex flex-1 flex-col gap-2">
			{#each OUTPUTS as o (o.branch)}
				<OutputRow
					{nodeId}
					branch={o.branch}
					label={o.label}
					open={data.selected === o.branch}
					onSelect={wired ? null : () => select(o.branch)}
					live={!isPreview} />
			{/each}
		</div>
	</div>
</Wrapper>
