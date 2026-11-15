<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import Toc from './toc.svelte';

	const { Story } = defineMeta({ title: 'Layout/Toc', component: Toc, tags: ['autodocs'] });

	const items = [
		{ id: 'intro', label: 'Introduction', level: 2 },
		{ id: 'setup', label: 'Setup', level: 2 },
		{ id: 'setup-mac', label: 'macOS', level: 3 },
		{ id: 'setup-win', label: 'Windows', level: 3 },
		{ id: 'usage', label: 'Usage', level: 2 }
	];
</script>

<script lang="ts">
	let active = $state('setup');
</script>

<Story name="Default">
	{#snippet template()}
		<div class="flex w-56 flex-col gap-4 p-6">
			<Toc {items} activeId={active} />
			<div class="flex gap-1">
				{#each items as i (i.id)}<button class="rounded border px-1 text-[10px]" onclick={() => (active = i.id)}>{i.id}</button>{/each}
			</div>
		</div>
	{/snippet}
</Story>
