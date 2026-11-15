<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import IconCopy from '~icons/fluent/copy-16-filled';
	import IconDelete from '~icons/fluent/delete-16-filled';
	import Menu from './menu.svelte';
	import MenuItem from './menu-item.svelte';
	import MenuSection from './menu-section.svelte';
	import MenuSeparator from './menu-separator.svelte';

	const { Story } = defineMeta({ title: 'Overlay/Menu', component: Menu, tags: ['autodocs'] });
</script>

<Story name="Default">
	{#snippet template()}
		<div class="w-56 p-8">
			<Menu>
				<MenuSection label="Edit" />
				<MenuItem label="Copy" icon={IconCopy}>
					{#snippet shortcut()}Cmd C{/snippet}
				</MenuItem>
				<MenuItem label="Disabled" disabled />
				<MenuSeparator />
				<MenuItem label="Delete" icon={IconDelete} danger />
			</Menu>
		</div>
	{/snippet}
</Story>
