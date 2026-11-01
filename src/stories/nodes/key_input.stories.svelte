<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import NodeCanvas from '../harness/node_canvas.svelte';
	import { compose, dataArgs } from '../harness/args';

	const { argTypes, args } = compose(dataArgs({ hotkey: { control: 'select', options: ['', 'F13', 'Command+Shift+KeyK'] } }, { hotkey: '' }));

	const { Story } = defineMeta({ title: 'Nodes/Conditions/Key Input', argTypes, args });
</script>

{#snippet template(a: Record<string, unknown>)}
	<NodeCanvas kind="keyInput" args={a} />
{/snippet}

<Story name="Unbound" {template} />
<Story name="Bound" args={{ hotkey: 'F13' }} {template} />
<Story name="Combo" args={{ hotkey: 'Command+Shift+KeyK' }} {template} />
