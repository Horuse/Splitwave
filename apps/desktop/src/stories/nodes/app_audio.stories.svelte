<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import NodeCanvas from '../harness/node_canvas.svelte';
	import { compose, dataArgs, envArgs, range, signalArgs } from '../harness/args';

	const { argTypes, args } = compose(
		dataArgs(
			{ bundleId: { control: 'select', options: ['com.example.app1', 'com.example.app2', 'com.example.gone'] }, volume: { control: range(0, 1, 0.01) } },
			{ bundleId: 'com.example.app1', volume: 1 }
		),
		signalArgs(['signal', 'signalLevel', 'signalChannels', 'signalMotion']),
		envArgs(['running', 'apps', 'deviceChannels', 'deviceSampleRate', 'pipelineSampleRate'])
	);

	const { Story } = defineMeta({ title: 'Nodes/Inputs/App Audio', argTypes, args });
</script>

{#snippet template(a: Record<string, unknown>)}
	<NodeCanvas kind="appAudio" args={a} />
{/snippet}

<Story name="Default" {template} />
<Story name="Nothing selected" args={{ bundleId: null }} {template} />
<Story name="No apps playing" args={{ bundleId: null, apps: 0 }} {template} />
<Story name="App closed" args={{ bundleId: 'com.example.gone' }} {template} />
<Story name="Many apps" args={{ apps: 8 }} {template} />
<Story name="Muted" args={{ volume: 0 }} {template} />
