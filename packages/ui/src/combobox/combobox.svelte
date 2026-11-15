<script lang="ts">
	import type { Snippet } from 'svelte';
	import IconChevronDown from '~icons/fluent/chevron-down-16-filled';

	interface Option {
		value: string;
		label: string;
		/** Optional base64-encoded PNG (no data: prefix); rendered as a 16×16 icon. */
		icon?: string | null;
		/** Secondary line under the label (e.g. a plugin vendor). Also searched. */
		subtitle?: string | null;
		/** Short leading tag (e.g. a plugin format). Also searched. */
		badge?: string | null;
	}

	interface Props {
		options: Option[];
		value: string | null;
		placeholder?: string;
		emptyHint?: string;
		onChange: (v: string | null) => void;
		class?: string;
		/** Compact variant for dense node UIs. */
		size?: 'sm' | 'md';
		/** Footer pinned below the list, e.g. `ComboboxAction` / `RescanButton`
		 * rows. Receives a `close` callback so an action can dismiss the panel. */
		footer?: Snippet<[() => void]>;
		/** Fired when the panel opens, so the owner can re-scan its option source. */
		onOpen?: () => void;
	}

	let { options, value, placeholder = '— Select —', emptyHint = 'No matches', onChange, size = 'md', class: ClassName, footer, onOpen }: Props = $props();

	let open = $state(false);
	let search = $state('');
	let containerEl = $state<HTMLDivElement>();
	let inputEl = $state<HTMLInputElement>();
	let activeIndex = $state(0);

	let selected = $derived(options.find((o) => o.value === value));
	let filtered = $derived.by(() => {
		const q = search.trim().toLowerCase();
		if (!q) return options;
		return options.filter((o) => `${o.label} ${o.subtitle ?? ''} ${o.badge ?? ''}`.toLowerCase().includes(q));
	});

	function openPanel() {
		open = true;
		onOpen?.();
		search = '';
		activeIndex = Math.max(
			0,
			filtered.findIndex((o) => o.value === value)
		);
		setTimeout(() => inputEl?.focus(), 0);
	}

	function closePanel() {
		open = false;
		search = '';
	}

	function pick(opt: Option | null) {
		onChange(opt?.value ?? null);
		closePanel();
	}

	function onKeyDown(e: KeyboardEvent) {
		if (e.key === 'Escape') {
			e.preventDefault();
			closePanel();
		} else if (e.key === 'ArrowDown') {
			e.preventDefault();
			activeIndex = Math.min(filtered.length - 1, activeIndex + 1);
		} else if (e.key === 'ArrowUp') {
			e.preventDefault();
			activeIndex = Math.max(0, activeIndex - 1);
		} else if (e.key === 'Enter') {
			e.preventDefault();
			const opt = filtered[activeIndex];
			if (opt) pick(opt);
		}
	}

	$effect(() => {
		if (!open) return;
		const onDocPointer = (e: PointerEvent) => {
			if (containerEl && !containerEl.contains(e.target as Node)) closePanel();
		};
		document.addEventListener('pointerdown', onDocPointer);
		return () => document.removeEventListener('pointerdown', onDocPointer);
	});
</script>

<div bind:this={containerEl} class={['relative min-w-0', ClassName]}>
	<button type="button" class={['combobox-trigger', size === 'sm' && 'combobox-sm']} aria-expanded={open} onclick={() => (open ? closePanel() : openPanel())}>
		<span class="flex min-w-0 flex-1 items-center gap-1.5">
			{#if selected?.icon}
				<img src="data:image/png;base64,{selected.icon}" alt="" class="h-4 w-4 shrink-0" />
			{/if}
			{#if selected?.badge}
				<span class="combobox-badge">
					{selected.badge}
				</span>
			{/if}
			<span class={['truncate', !selected && 'text-subtle']}>
				{selected?.label ?? placeholder}
			</span>
		</span>
		<IconChevronDown class="size-3 shrink-0 opacity-60" />
	</button>

	{#if open}
		<div class="combobox-panel">
			<input bind:this={inputEl} bind:value={search} type="text" class="combobox-filter" placeholder="Filter…" onkeydown={onKeyDown} />
			<ul class="nowheel max-h-48 overflow-y-auto">
				{#if value !== null}
					<li>
						<button type="button" class="combobox-option text-xs" onclick={() => pick(null)}> — Clear selection — </button>
					</li>
				{/if}
				{#each filtered as opt, i (opt.value)}
					<li>
						<button
							type="button"
							class="combobox-option"
							data-active={i === activeIndex}
							aria-selected={opt.value === value}
							onmouseenter={() => (activeIndex = i)}
							onclick={() => pick(opt)}>
							{#if opt.icon}
								<img src="data:image/png;base64,{opt.icon}" alt="" class="h-4 w-4 shrink-0" />
							{/if}
							{#if opt.badge}
								<span class="combobox-badge">
									{opt.badge}
								</span>
							{/if}
							<span class="flex min-w-0 flex-col">
								<span class="truncate">{opt.label}</span>
								{#if opt.subtitle}
									<span class="truncate text-[10px] text-subtle">{opt.subtitle}</span>
								{/if}
							</span>
						</button>
					</li>
				{/each}
				{#if filtered.length === 0}
					<li class="px-2 py-1 text-xs text-subtle italic">{emptyHint}</li>
				{/if}
			</ul>
			{#if footer}
				{@render footer(closePanel)}
			{/if}
		</div>
	{/if}
</div>
