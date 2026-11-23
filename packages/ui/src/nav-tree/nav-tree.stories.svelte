<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { Sidebar } from '../sidebar';
	import NavTree from './nav-tree.svelte';
	import type { NavNode } from './types';

	const { Story } = defineMeta({ title: 'Layout/Nav Tree', component: NavTree, tags: ['autodocs'], parameters: { layout: 'fullscreen' } });

	const items: NavNode[] = [
		{ id: 'start', label: 'Getting started' },
		{
			id: 'canvas',
			label: 'Canvas',
			children: [
				{ id: 'canvas/connect', label: 'Connecting channels' },
				{
					id: 'canvas/selection',
					label: 'Selection',
					children: [
						{ id: 'canvas/selection/box', label: 'Box select' },
						{ id: 'canvas/selection/copy', label: 'Copy and paste' }
					]
				}
			]
		}
	];
</script>

<Story name="Nested">
	{#snippet template()}
		<div class="flex h-[28rem] bg-background">
			<Sidebar><NavTree {items} activeId="canvas/selection/box" /></Sidebar>
		</div>
	{/snippet}
</Story>
