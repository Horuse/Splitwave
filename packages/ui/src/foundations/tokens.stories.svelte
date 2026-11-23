<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';

	const { Story } = defineMeta({ title: 'Foundations/Palette', tags: ['autodocs'] });

	const steps = [50, 100, 200, 300, 400, 500, 600, 700, 750, 800, 900, 1000, 1100, 1200];
</script>

<Story name="Neutral">
	{#snippet template()}
		<div class="flex flex-wrap gap-3 bg-background p-6">
			{#each steps as step (step)}
				<div class="flex flex-col gap-1">
					<div class="h-10 w-20 rounded-md border border-neutral-400" style:background-color="var(--color-neutral-{step})"></div>
					<span class="font-mono text-[11px] text-neutral-900">{step}</span>
				</div>
			{/each}
		</div>
	{/snippet}
</Story>
