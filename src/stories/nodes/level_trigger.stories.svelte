<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import NodeCanvas from '../harness/node_canvas.svelte';
	import { compose, dataArgs, range, signalArgs, wiringArgs } from '../harness/args';

	const { argTypes, args } = compose(
		dataArgs({ thresholdDb: { control: range(-60, 0, 1) }, holdMs: { control: range(0, 2000, 10) } }, { thresholdDb: -40, holdMs: 300 }),
		signalArgs(['signal', 'signalLevel', 'signalMotion']),
		wiringArgs(2)
	);

	const { Story } = defineMeta({ title: 'Nodes/Conditions/Level Trigger', argTypes, args });
</script>

{#snippet template(a: Record<string, unknown>)}
	<NodeCanvas kind="levelTrigger" args={a} />
{/snippet}

<Story name="Above threshold" {template} />
<Story name="Below threshold" args={{ thresholdDb: -3 }} {template} />
<Story name="Speech-like" args={{ signalMotion: true, holdMs: 500 }} {template} />
<Story name="Silent" args={{ signal: 'off' }} {template} />
