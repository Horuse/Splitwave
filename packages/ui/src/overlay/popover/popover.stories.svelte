<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { Button } from '../../button';
	import Popover from './popover.svelte';

	const { Story } = defineMeta({ title: 'Overlay/Popover', component: Popover, tags: ['autodocs'] });
</script>

<Story name="Default">
	{#snippet template()}
		<div class="p-16">
			<Popover>
				{#snippet trigger()}
					<Button>Open popover</Button>
				{/snippet}
				<div class="w-56 rounded-lg border border-line-strong bg-surface-2 p-3 text-xs">Popover content. Click outside or press Escape to close.</div>
			</Popover>
		</div>
	{/snippet}
</Story>
