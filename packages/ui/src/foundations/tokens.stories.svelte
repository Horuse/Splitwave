<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';

	const { Story } = defineMeta({
		title: 'Foundations/Tokens',
		tags: ['autodocs']
	});

	const surfaces = ['surface-0', 'surface-1', 'surface-2', 'surface-3', 'surface-4'];
	const text = ['content', 'muted', 'subtle'];
	const accents = ['accent', 'accent-hover', 'accent-soft', 'danger', 'success', 'warning', 'info', 'signal'];
	const cats = ['input', 'output', 'monitor', 'network', 'effect', 'control', 'routing'].map((c) => `cat-${c}`);
</script>

{#snippet swatch(name: string)}
	<div class="flex flex-col gap-1">
		<div class="h-10 w-24 rounded-md border border-line" style:background-color="var(--color-{name})"></div>
		<span class="font-mono text-[11px] text-muted">{name}</span>
	</div>
{/snippet}

<Story name="Palette">
	<div class="flex flex-col gap-6 bg-surface-0 p-6">
		{#each [['Surfaces', surfaces], ['Text', text], ['Accent and status', accents], ['Node categories', cats]] as [title, names] (title)}
			<section class="flex flex-col gap-2">
				<h3 class="text-xs font-semibold text-muted uppercase">{title}</h3>
				<div class="flex flex-wrap gap-3">
					{#each names as name (name)}
						{@render swatch(name as string)}
					{/each}
				</div>
			</section>
		{/each}
	</div>
</Story>

<Story name="Layers">
	<div data-layer="0" class="flex gap-4 p-6">
		{#each [1, 2, 3] as level (level)}
			<div data-layer={level} class="flex flex-col gap-2 rounded-lg border border-layer-line p-4">
				<span class="font-mono text-[11px] text-muted">layer {level}</span>
				<div class="rounded-md border border-layer-next-line bg-layer-next px-3 py-1.5 text-xs">control</div>
			</div>
		{/each}
	</div>
</Story>

<Story name="Typography">
	<div class="flex flex-col gap-2 bg-surface-1 p-6">
		<div class="text-base font-semibold">Splitwave Inter 16 semibold</div>
		<div class="text-sm">Pipeline routing Inter 13 regular</div>
		<div class="font-mono text-xs tabular-nums">-12.4 dB 48000 Hz 0.00 1.00</div>
	</div>
</Story>
