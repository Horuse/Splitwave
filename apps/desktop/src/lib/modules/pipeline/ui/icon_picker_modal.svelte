<script lang="ts">
	import { untrack } from 'svelte';
	import { Button, ScrollArea } from '@splitwave/ui';
	import { modalManager, type ModalBaseProps } from '$lib/modules/overlay/modal';
	import { iconLabel, searchIcons } from '../icons/search';
	import { iconStore } from '../icons/store.svelte';
	import IconGlyph from './icon_glyph.svelte';
	import Search from '~icons/fluent/search-16-filled';

	/** Resolves with a name, `null` to remove the icon, `undefined` when dismissed. */
	let { modalId, current, initialQuery = '' }: ModalBaseProps & { current?: string; initialQuery?: string } = $props();

	const COLUMNS = 10;
	// A 44px cell and the 4px gap under it.
	const ROW_PX = 48;
	const VIEW_PX = 344;
	const OVERSCAN_ROWS = 3;

	let query = $state(untrack(() => initialQuery));
	let viewport = $state<HTMLElement | null>(null);
	let scrollTop = $state(0);
	let loadError = $state<string | null>(null);

	iconStore.load().catch((e) => (loadError = String(e)));

	const matches = $derived(searchIcons(iconStore.names, query));
	const rows = $derived(Math.ceil(matches.length / COLUMNS));
	// Only the rows in view exist: the full set is thousands of SVGs.
	const firstRow = $derived(Math.max(0, Math.floor(scrollTop / ROW_PX) - OVERSCAN_ROWS));
	const lastRow = $derived(Math.min(rows, Math.ceil((scrollTop + VIEW_PX) / ROW_PX) + OVERSCAN_ROWS));
	const visible = $derived(matches.slice(firstRow * COLUMNS, lastRow * COLUMNS));

	$effect(() => {
		const element = viewport;
		if (!element) return;
		const onScroll = () => (scrollTop = element.scrollTop);
		element.addEventListener('scroll', onScroll, { passive: true });
		return () => element.removeEventListener('scroll', onScroll);
	});

	function onSearch() {
		viewport?.scrollTo({ top: 0 });
		scrollTop = 0;
	}
</script>

<div class="flex flex-col gap-3 px-5 pt-3 pb-5">
	<label class="flex items-center gap-2 rounded-lg border border-neutral-400 bg-neutral-200 px-3 transition-colors focus-within:border-neutral-600">
		<Search class="size-4 shrink-0 text-neutral-900" />
		<!-- svelte-ignore a11y_autofocus -->
		<input
			bind:value={query}
			oninput={onSearch}
			autofocus
			placeholder="Search: headphones, mic, guitar"
			aria-label="Search icons"
			class="min-w-0 flex-1 bg-transparent py-2 text-sm text-theme placeholder:text-neutral-900 focus:outline-none" />
		<span class="shrink-0 text-xs text-neutral-900 tabular-nums">{matches.length || ''}</span>
	</label>

	<div class="overflow-hidden rounded-xl border border-neutral-300 bg-background">
		<ScrollArea class="h-[344px]" viewportClass="h-full" bind:viewport>
			{#if loadError}
				<p role="alert" class="px-4 py-16 text-center text-xs text-red-500">{loadError}</p>
			{:else if !iconStore.names.length}
				<p class="px-4 py-16 text-center text-xs text-neutral-900">Loading…</p>
			{:else if !matches.length}
				<p class="px-4 py-16 text-center text-xs text-neutral-900">No icons match "{query}". Names are in English.</p>
			{:else}
				<div class="p-2">
					<div class="relative" style:height="{rows * ROW_PX}px">
						<div class="absolute inset-x-0 grid auto-rows-[2.75rem] grid-cols-10 gap-1" style:top="{firstRow * ROW_PX}px">
							{#each visible as name (name)}
								{@const svg = iconStore.svg(name)}
								<!-- Not `Button`: each of its variants is a filled tile, and a grid of them reads as one block. -->
								<button
									type="button"
									class={[
										'flex cursor-pointer items-center justify-center rounded-lg border',
										name === current
											? 'border-brand/50 bg-brand/15 text-brand'
											: 'border-transparent text-neutral-1000 hover:bg-neutral-300 hover:text-theme'
									]}
									aria-label={iconLabel(name)}
									aria-pressed={name === current}
									title={iconLabel(name)}
									onclick={() => modalManager.close(modalId, name)}>
									{#if svg}<IconGlyph {svg} class="size-6" />{/if}
								</button>
							{/each}
						</div>
					</div>
				</div>
			{/if}
		</ScrollArea>
	</div>

	{#if current}
		<div class="flex justify-end">
			<Button onclick={() => modalManager.close(modalId, null)}>Remove icon</Button>
		</div>
	{/if}
</div>
