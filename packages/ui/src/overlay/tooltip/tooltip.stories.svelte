<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { Button } from '../../button';
	import Tooltip from './tooltip.svelte';

	const { Story } = defineMeta({
		title: 'Overlay/Tooltip',
		component: Tooltip,
		tags: ['autodocs'],
		argTypes: { placement: { control: 'select', options: ['top', 'right', 'bottom', 'left'] } }
	});
</script>

<Story name="Default">
	{#snippet template()}
		<div class="flex gap-6 p-16">
			<Tooltip text="Top tooltip"><Button>Hover me</Button></Tooltip>
			<Tooltip text="Right tooltip" placement="right"><Button>Right</Button></Tooltip>
			<Tooltip text="Bottom tooltip" placement="bottom"><Button>Bottom</Button></Tooltip>
		</div>
	{/snippet}
</Story>
