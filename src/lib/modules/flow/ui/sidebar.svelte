<script lang="ts">
	import type { NodeCategory, NodeKind } from '@splitwave/nodes';
	import { Button, Input, Sidebar, SidebarSection, smoothRadius } from '@splitwave/ui';
	import { pipelineStore } from '$lib/modules/pipeline/stores.svelte';
	import { audioStore } from '$lib/modules/audio/stores.svelte';
	import { ActivationButton, LatencyBadge, PipelineRateBadge, RunningTimer } from '$lib/modules/audio/ui';
	import { DND_MIME } from '../utils/context';
	import { categoryLabel, categoryOrder, kindsByCategory, registry } from '../utils/nodes';
	import Add from '~icons/fluent/add-24-filled';
	import Timer from '~icons/fluent/timer-16-filled';
	import ArrowSwap from '~icons/fluent/arrow-swap-16-filled';
	import DataBar from '~icons/fluent/data-bar-vertical-16-filled';
	import KeyCommand from '~icons/fluent/key-command-24-regular';
	import Mic from '~icons/fluent/mic-16-filled';
	import Plug from '~icons/fluent/plug-connected-16-filled';
	import Sliders from '~icons/fluent/options-16-filled';
	import Speaker from '~icons/fluent/speaker-2-16-filled';
	import { CATEGORY_TEXT } from '../utils/accents';

	let { pipelineId }: { pipelineId: string } = $props();

	const CATEGORY_ICON = {
		input: Mic,
		output: Speaker,
		monitor: DataBar,
		network: Plug,
		effect: Sliders,
		control: KeyCommand,
		routing: ArrowSwap
	};

	let query = $state('');
	let sections: Partial<Record<NodeCategory, HTMLElement>> = {};

	let visible = $derived.by(() => {
		const q = query.trim().toLowerCase();
		return categoryOrder
			.map((category) => ({
				category,
				kinds: kindsByCategory[category].filter((kind) => {
					if (!q) return true;
					const n = registry[kind];
					return n.label.toLowerCase().includes(q) || n.description.toLowerCase().includes(q);
				})
			}))
			.filter((g) => g.kinds.length > 0);
	});

	function jumpTo(category: NodeCategory) {
		query = '';
		requestAnimationFrame(() => sections[category]?.scrollIntoView({ behavior: 'smooth', block: 'start' }));
	}

	function onDragStart(event: DragEvent, kind: NodeKind) {
		if (!event.dataTransfer) return;
		event.dataTransfer.setData(DND_MIME, kind);
		event.dataTransfer.effectAllowed = 'move';
	}

	function onClickAdd(kind: NodeKind) {
		pipelineStore.editorActions?.addNode(kind);
	}
</script>

<Sidebar layer="page" class="w-72 border-r-0 border-l">
	{#snippet header()}
		<div class="flex flex-col gap-2 pb-1">
			<Input bind:value={query} placeholder="Search nodes" />
			<div class="flex gap-1">
				{#each categoryOrder as category (category)}
					{@const Icon = CATEGORY_ICON[category]}
					<Button
						variant="ghost"
						icon
						class={['flex-1', CATEGORY_TEXT[category]]}
						title={categoryLabel[category]}
						aria-label={`Jump to ${categoryLabel[category]}`}
						onclick={() => jumpTo(category)}>
						<Icon class="size-4" />
					</Button>
				{/each}
			</div>
		</div>
	{/snippet}

	{#each visible as { category, kinds } (category)}
		<section bind:this={sections[category]} class="flex scroll-mt-2 flex-col gap-1.5">
			<SidebarSection label={categoryLabel[category]} />
			{#each kinds as kind (kind)}
				{@const node = registry[kind]}
				<div
					role="listitem"
					draggable="true"
					ondragstart={(e) => onDragStart(e, kind)}
					class="flex cursor-grab items-start gap-2.5 bg-(--lv-1) px-3 py-2 transition-colors duration-150 hover:bg-(--lv-2)"
					style="border-radius: var(--r, 10px)"
					{@attach smoothRadius({ variable: '--r' })}>
					<node.icon class={['mt-0.5 size-4 shrink-0', CATEGORY_TEXT[node.category]]} />
					<div class="flex min-w-0 flex-1 flex-col gap-0.5">
						<span class="text-sm font-medium text-theme">{node.label}</span>
						<span class="text-[11px] leading-snug text-neutral-900">{node.description}</span>
					</div>
					<Button variant="ghost" icon size="sm" aria-label={`Add ${node.label}`} title={`Add ${node.label}`} onclick={() => onClickAdd(kind)}>
						<Add class="size-3.5" />
					</Button>
				</div>
			{/each}
		</section>
	{:else}
		<p class="px-2.5 py-4 text-xs text-neutral-900">No nodes match "{query}".</p>
	{/each}

	{#snippet footer()}
		<div class="flex flex-col gap-2">
			<div class="flex items-center gap-2">
				<PipelineRateBadge />
				{#if audioStore.isRunning}
					<LatencyBadge />
					<span
						class="flex h-7 items-center gap-1.5 border border-(--lv-3) bg-(--lv-1) px-2.5 text-xs text-brand"
						style="border-radius: var(--r, 10px)"
						{@attach smoothRadius({ variable: '--r' })}>
						<Timer class="size-4" />
						<RunningTimer />
					</span>
				{/if}
			</div>
			<ActivationButton {pipelineId} size="lg" class="w-full" />
		</div>
	{/snippet}
</Sidebar>
