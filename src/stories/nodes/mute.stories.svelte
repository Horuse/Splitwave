<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import NodeCanvas from '../harness/node_canvas.svelte';
	import ControlCanvas from '../harness/control_canvas.svelte';
	import { compose, dataArgs, envArgs, range, wiringArgs } from '../harness/args';
	import { CONTROL_OUT, OPEN_HANDLE } from '$lib/modules/flow/utils';

	const { argTypes, args } = compose(
		dataArgs(
			{
				muted: { control: 'boolean' },
				bypassed: { control: 'boolean' },
				cueEnabled: { control: 'boolean' },
				cueDeviceId: { control: 'select', options: ['out-1', 'out-2', 'missing-device'] },
				cueVolume: { control: range(0, 100, 1) }
			},
			{ muted: false, bypassed: false, cueEnabled: false, cueDeviceId: 'out-1', cueVolume: 40 }
		),
		wiringArgs(2),
		envArgs(['outputDevices'])
	);

	const { Story } = defineMeta({ title: 'Nodes/Effects/Mute', argTypes, args });
</script>

{#snippet template(a: Record<string, unknown>)}
	<NodeCanvas kind="mute" args={a} />
{/snippet}

{#snippet pushToTalk()}
	<ControlCanvas from="keyInput" fromData={{ hotkey: 'F13' }} fromHandle={CONTROL_OUT} to="mute" toHandle={OPEN_HANDLE} />
{/snippet}

<Story name="Default" {template} />
<Story name="Muted" args={{ muted: true }} {template} />
<Story name="Push to talk" template={pushToTalk} />
<Story name="Cue enabled" args={{ cueEnabled: true }} {template} />
<Story name="Cue device missing" args={{ cueEnabled: true, cueDeviceId: 'missing-device' }} {template} />
<Story name="Bypassed" args={{ bypassed: true }} {template} />
