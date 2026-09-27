<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import NodeCanvas from '../harness/node_canvas.svelte';
	import { compose, dataArgs, envArgs, range, wiringArgs } from '../harness/args';

	const { argTypes, args } = compose(
		dataArgs(
			{
				muted: { control: 'boolean' },
				bypassed: { control: 'boolean' },
				hotkey: { control: 'select', options: ['', 'CommandOrControl+Shift+M', 'F13'] },
				pushToTalk: { control: 'boolean' },
				cueEnabled: { control: 'boolean' },
				cueDeviceId: { control: 'select', options: ['out-1', 'out-2', 'missing-device'] },
				cueVolume: { control: range(0, 100, 1) }
			},
			{ muted: false, bypassed: false, hotkey: '', pushToTalk: false, cueEnabled: false, cueDeviceId: 'out-1', cueVolume: 40 }
		),
		wiringArgs(2),
		envArgs(['outputDevices'])
	);

	const { Story } = defineMeta({ title: 'Nodes/Effects/Mute', argTypes, args });
</script>

{#snippet template(a: Record<string, unknown>)}
	<NodeCanvas kind="mute" args={a} />
{/snippet}

<Story name="Default" {template} />
<Story name="Muted" args={{ muted: true }} {template} />
<Story name="With hotkey" args={{ hotkey: 'CommandOrControl+Shift+M' }} {template} />
<Story name="Push to talk" args={{ hotkey: 'F13', pushToTalk: true, muted: true }} {template} />
<Story name="Cue enabled" args={{ cueEnabled: true }} {template} />
<Story name="Cue device missing" args={{ cueEnabled: true, cueDeviceId: 'missing-device' }} {template} />
<Story name="Bypassed" args={{ bypassed: true }} {template} />
