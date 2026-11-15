<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import ScrollArea from './scroll-area.svelte';
	import Card from '../card/card.svelte';

	const { Story } = defineMeta({ title: 'Layout/ScrollArea', component: ScrollArea, tags: ['autodocs'] });
</script>

<Story name="Default">
	{#snippet template()}
		<div class="h-80 w-72 bg-background p-2">
			<ScrollArea class="h-full" viewportClass="flex flex-col gap-2">
				{#each Array(14) as _, i (i)}
					<Card class="p-3 text-sm">Card {i + 1}</Card>
				{/each}
			</ScrollArea>
		</div>
	{/snippet}
</Story>

<Story name="Sizes">
	{#snippet template()}
		<div class="flex gap-6 bg-background p-6">
			{#each ['sm', 'md', 'lg', 'xl'] as const as size (size)}
				<div class="flex flex-col gap-2">
					<span class="text-sm text-neutral-900">{size}</span>
					<ScrollArea {size} visibility="always" class="h-64 w-48" viewportClass="flex flex-col gap-2">
						{#each Array(12) as _, i (i)}
							<Card class="p-3 text-sm">Card {i + 1}</Card>
						{/each}
					</ScrollArea>
				</div>
			{/each}
		</div>
	{/snippet}
</Story>
