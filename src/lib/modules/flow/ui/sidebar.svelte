<script lang="ts">
	import type { NodeCategory, NodeKind } from '$lib/modules/pipeline/types';
	import { pipelineStore } from '$lib/modules/pipeline/stores.svelte';
	import {
		DND_MIME,
		categoryLabel,
		categoryOrder,
		kindsByCategory,
		registry
	} from '../utils/nodes';
	import { Add, DataBar, Mic, Plug, Sliders, Speaker } from '$lib/components/icons';

	const ACCENT_TEXT: Record<NodeCategory, string> = {
		input: 'text-emerald-600 dark:text-emerald-400',
		output: 'text-sky-600 dark:text-sky-400',
		monitor: 'text-amber-600 dark:text-amber-400',
		network: 'text-rose-600 dark:text-rose-400',
		effect: 'text-violet-600 dark:text-violet-400'
	};

	const CATEGORY_ICON = {
		input: Mic,
		output: Speaker,
		monitor: DataBar,
		network: Plug,
		effect: Sliders
	};

	let sections: Partial<Record<NodeCategory, HTMLElement>> = {};

	function jumpTo(category: NodeCategory) {
		sections[category]?.scrollIntoView({ behavior: 'smooth', block: 'start' });
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

<aside class="flex w-72 flex-col border-l border-neutral-100 bg-background">
	<nav
		class="flex shrink-0 items-center gap-1 border-b border-neutral-100 bg-background px-4 py-2.5"
	>
		{#each categoryOrder as category (category)}
			{@const Icon = CATEGORY_ICON[category]}
			<button
				type="button"
				onclick={() => jumpTo(category)}
				title={categoryLabel[category]}
				aria-label={`Jump to ${categoryLabel[category]}`}
				class={[
					'flex flex-1 items-center justify-center rounded-lg bg-neutral-100 py-1.5 transition-colors hover:bg-neutral-300',
					ACCENT_TEXT[category]
				]}
			>
				<Icon class="size-4" />
			</button>
		{/each}
	</nav>

	<div class="flex flex-1 flex-col gap-5 overflow-y-auto p-4">
		{#each categoryOrder as category (category)}
			<section bind:this={sections[category]} class="flex scroll-mt-4 flex-col gap-2">
				<h2 class="text-[10px] font-semibold tracking-wider text-neutral-1000 uppercase">
					{categoryLabel[category]}
				</h2>
				<ul class="flex flex-col gap-1.5">
					{#each kindsByCategory[category] as kind (kind)}
						{@const node = registry[kind]}
						<li
							draggable="true"
							ondragstart={(e) => onDragStart(e, kind)}
							class="group flex items-start justify-between gap-2 rounded-lg bg-neutral-100 px-3 py-2 hover:bg-neutral-200"
						>
							<div class="flex min-w-0 items-start gap-2">
								<node.icon class={['mt-0.5 size-4 shrink-0', ACCENT_TEXT[node.category]]} />
								<div class="flex min-w-0 flex-col">
									<span class="text-sm font-medium text-theme">{node.label}</span>
									<span class="text-[11px] leading-tight text-neutral-900">
										{node.description}
									</span>
								</div>
							</div>
							<button
								class="flex h-6 w-6 shrink-0 items-center justify-center rounded text-neutral-1000 hover:bg-neutral-300"
								onclick={() => onClickAdd(kind)}
								aria-label={`Add ${node.label}`}
							>
								<Add class="h-4 w-4" />
							</button>
						</li>
					{/each}
				</ul>
			</section>
		{/each}

		<p class="mt-auto text-[10px] text-neutral-900">Click + to add, or drag onto canvas.</p>
	</div>
</aside>
