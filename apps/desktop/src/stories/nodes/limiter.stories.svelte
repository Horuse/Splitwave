<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import NodeCanvas from '../harness/node_canvas.svelte';
	import { compose, dataArgs, range, signalArgs, wiringArgs } from '../harness/args';

	const { argTypes, args } = compose(
		dataArgs(
			{
				ceilingDb: { control: range(-12, 0, 0.1) },
				lookaheadMs: { control: range(1, 20, 0.5) },
				releaseMs: { control: range(10, 500, 5) },
				bypassed: { control: 'boolean' }
			},
			{ ceilingDb: -0.3, lookaheadMs: 5, releaseMs: 50, bypassed: false }
		),
		wiringArgs(2),
		signalArgs(['signal', 'signalLevel', 'signalChannels', 'gainReduction', 'signalMotion'])
	);

	const { Story } = defineMeta({ title: 'Nodes/Effects/Limiter', argTypes, args });
</script>

{#snippet template(a: Record<string, unknown>)}
	<NodeCanvas kind="limiter" args={a} />
{/snippet}

<Story name="Default" {template} />
<Story name="Idle" args={{ gainReduction: 0 }} {template} />
<Story name="Heavy limiting" args={{ gainReduction: 14, ceilingDb: -6 }} {template} />
<Story name="Bypassed" args={{ bypassed: true }} {template} />
