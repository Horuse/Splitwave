<script lang="ts" generics="T">
	interface Option {
		label: string;
		subtitle?: string;
		value: T;
		disabled?: boolean;
	}

	let {
		options,
		value,
		onSelect,
		label,
		note,
		columns
	}: {
		options: Option[];
		value: T;
		onSelect: (value: T) => void;
		label?: string;
		note?: string;
		columns?: number;
	} = $props();

	let cols = $derived(columns ?? options.length);
	let rows = $derived(Math.ceil(options.length / cols));
	let selectedIndex = $derived(options.findIndex((o) => o.value === value));
	let col = $derived(selectedIndex % cols);
	let row = $derived(Math.floor(selectedIndex / cols));
</script>

{#if label || note}
	<div class="flex items-baseline justify-between gap-2">
		{#if label}<span class="font-mono text-[9px] text-neutral-500">{label}</span>{/if}
		{#if note}<span class="font-mono text-[9px] text-neutral-400">{note}</span>{/if}
	</div>
{/if}

<div class="nodrag nopan relative rounded-md border border-neutral-300 bg-neutral-100 p-0.5">
	{#if selectedIndex >= 0}
		<!-- Offsets, not transform: a composited layer rasterises blurry under xyflow's zoom. -->
		<div class="pointer-events-none absolute inset-0.5">
			<div
				class="absolute rounded-sm bg-neutral-900 transition-[left,top] duration-200 ease-out"
				style="width:{100 / cols}%; height:{100 / rows}%; left:{(col * 100) / cols}%; top:{(row * 100) / rows}%;"
			></div>
		</div>
	{/if}

	<div class="relative grid" style="grid-template-columns: repeat({cols}, minmax(0, 1fr));">
		{#each options as opt (opt.label)}
			<button
				type="button"
				disabled={opt.disabled}
				onclick={() => onSelect(opt.value)}
				title={opt.subtitle || opt.label}
				class={[
					'relative z-10 flex flex-col items-center justify-center rounded-sm px-1 py-1.5 leading-none transition-colors disabled:opacity-30',
					value === opt.value
						? 'text-white'
						: 'text-neutral-900 not-disabled:hover:bg-neutral-200/60'
				]}
			>
				<span class="font-mono text-[9px] tabular-nums">{opt.label}</span>
				{#if opt.subtitle}
					<span class="mt-0.5 text-[7px] opacity-70">{opt.subtitle}</span>
				{/if}
			</button>
		{/each}
	</div>
</div>
