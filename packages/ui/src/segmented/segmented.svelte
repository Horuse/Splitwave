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
		columns,
		class: className
	}: {
		options: Option[];
		value: T;
		onSelect: (value: T) => void;
		label?: string;
		note?: string;
		columns?: number;
		class?: string;
	} = $props();

	let cols = $derived(columns ?? options.length);
	let rows = $derived(Math.ceil(options.length / cols));
	let selectedIndex = $derived(options.findIndex((o) => o.value === value));
	let col = $derived(selectedIndex % cols);
	let row = $derived(Math.floor(selectedIndex / cols));
</script>

{#if label || note}
	<div class="flex items-baseline justify-between gap-2">
		{#if label}<span class="segmented-caption">{label}</span>{/if}
		{#if note}<span class="segmented-caption">{note}</span>{/if}
	</div>
{/if}

<div class={['segmented', className]}>
	{#if selectedIndex >= 0}
		<div class="pointer-events-none absolute inset-0.5">
			<div
				class="segmented-thumb"
				style="width:{100 / cols}%; height:{100 / rows}%; left:{(col * 100) / cols}%; top:{(row * 100) / rows}%;">
			</div>
		</div>
	{/if}

	<div class="relative grid" style="grid-template-columns: repeat({cols}, minmax(0, 1fr));">
		{#each options as opt (opt.value)}
			<button
				type="button"
				class="segmented-item"
				aria-pressed={value === opt.value}
				disabled={opt.disabled}
				onclick={() => onSelect(opt.value)}
				title={opt.subtitle || opt.label}>
				<span class="font-mono text-[9px] tabular-nums">{opt.label}</span>
				{#if opt.subtitle}
					<span class="mt-0.5 text-[7px] opacity-70">{opt.subtitle}</span>
				{/if}
			</button>
		{/each}
	</div>
</div>
