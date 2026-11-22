<script lang="ts">
	import { Panel } from '@xyflow/svelte';
	import { smoothRadius } from '@splitwave/ui';
	import { audioStore } from '$lib/modules/audio/stores.svelte';
	import { ActivationButton, LatencyBadge, PipelineRateBadge, RunningTimer } from '$lib/modules/audio/ui';
	import Timer from '~icons/fluent/timer-16-filled';

	let { pipelineId }: { pipelineId: string } = $props();
</script>

{#if audioStore.isRunning}
	<Panel position="top-left">
		<span
			class="flex h-7 items-center gap-1.5 border border-(--lv-3) bg-(--lv-1) px-2.5 text-xs text-brand"
			style="border-radius: var(--r, 10px)"
			{@attach smoothRadius({ variable: '--r' })}>
			<Timer class="size-4" />
			<RunningTimer />
		</span>
	</Panel>
{/if}

<Panel position="top-right" class="flex items-center gap-2">
	{#if audioStore.isRunning}
		<LatencyBadge />
	{/if}
	<PipelineRateBadge />
</Panel>

<Panel position="bottom-right">
	<ActivationButton {pipelineId} size="lg" opaque />
</Panel>
