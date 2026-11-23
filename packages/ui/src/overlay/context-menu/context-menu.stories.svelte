<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { Menu, MenuItem } from '../menu';
	import ContextMenu from './context-menu.svelte';

	const { Story } = defineMeta({ title: 'Overlay/Context Menu', component: ContextMenu, tags: ['autodocs'] });
</script>

<Story name="Default">
	{#snippet template()}
		<ContextMenu>
			{#snippet trigger()}
				<div class="border-line-strong text-muted m-8 flex h-40 w-80 items-center justify-center rounded-lg border border-dashed text-xs">
					Right click here
				</div>
			{/snippet}
			{#snippet menu({ close })}
				<Menu>
					<MenuItem label="Duplicate" onclick={close} />
					<MenuItem label="Delete" danger onclick={close} />
				</Menu>
			{/snippet}
		</ContextMenu>
	{/snippet}
</Story>
