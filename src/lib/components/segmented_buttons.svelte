<script lang="ts" generics="T">
	interface Option {
		label: string;
		subtitle?: string;
		value: T;
	}

	let {
		options,
		value,
		onSelect
	}: {
		options: Option[];
		value: T;
		onSelect: (value: T) => void;
	} = $props();

	let selectedIndex = $derived(options.findIndex((o) => o.value === value));
</script>

<div
	class="nodrag nopan relative grid gap-2 rounded-md border border-neutral-300 bg-neutral-100 p-0.5"
	style="grid-template-columns: repeat({options.length}, minmax(0, 1fr));"
>
	{#if selectedIndex >= 0}
		<div
			class="pointer-events-none absolute top-0.5 bottom-0.5 rounded-sm bg-neutral-900 transition-transform duration-200 ease-out"
			style="left: 2px; width: calc((100% - 4px - {(options.length - 1) * 8}px) / {options.length}); transform: translateX(calc({selectedIndex} * (100% + 8px)));"
		></div>
	{/if}
	{#each options as opt (opt.label)}
		<button
			type="button"
			onclick={() => onSelect(opt.value)}
			title={opt.subtitle || opt.label}
			class={[
				'relative z-10 flex flex-col items-center rounded-sm px-1 py-0.5 leading-none transition-colors',
				value === opt.value ? 'text-white' : 'text-neutral-900 hover:bg-neutral-200/60'
			]}
		>
			<span class="font-mono text-[9px] tabular-nums">{opt.label}</span>
			{#if opt.subtitle}
				<span class="text-[7px] opacity-70">{opt.subtitle}</span>
			{/if}
		</button>
	{/each}
</div>
