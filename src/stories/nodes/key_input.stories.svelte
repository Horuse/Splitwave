<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import NodeCanvas from '../harness/node_canvas.svelte';
	import { compose, dataArgs } from '../harness/args';

	const PAD = { device: 'Launchpad Mini', channel: 1, kind: 'note', number: 60 };
	const PEDAL = { device: 'FCB1010', channel: 1, kind: 'cc', number: 64 };

	const { argTypes, args } = compose(
		dataArgs(
			{
				source: { control: 'inline-radio', options: ['keyboard', 'midi'] },
				hotkey: { control: 'select', options: ['', 'F13', 'Command+Shift+KeyK'] }
			},
			{ source: 'keyboard', hotkey: '' }
		)
	);

	const { Story } = defineMeta({ title: 'Nodes/Conditions/Key Input', argTypes, args });
</script>

{#snippet template(a: Record<string, unknown>)}
	<NodeCanvas kind="keyInput" args={a} />
{/snippet}

<Story name="Unbound" {template} />
<Story name="Bound" args={{ hotkey: 'F13' }} {template} />
<Story name="Combo" args={{ hotkey: 'Command+Shift+KeyK' }} {template} />
<Story name="MIDI unbound" args={{ source: 'midi' }} {template} />
<Story name="MIDI note" args={{ source: 'midi', midi: PAD }} {template} />
<Story name="MIDI CC" args={{ source: 'midi', midi: PEDAL }} {template} />
