<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import ControlCanvas from '../harness/control_canvas.svelte';
	import { CONTROL_OUT, controlHandle } from '$lib/modules/flow/utils';

	const { Story } = defineMeta({
		title: 'Nodes/Conditions/Action Input',
		argTypes: { action: { control: 'inline-radio', options: ['stop', 'back', 'play', 'forward', 'loop'] } },
		args: { action: 'play' }
	});
</script>

{#snippet template(a: Record<string, any>)}
	{#key a.action}
		<ControlCanvas from="keyInput" fromData={{ hotkey: 'F13' }} fromHandle={CONTROL_OUT} to="audioFile" toHandle={controlHandle(a.action)} />
	{/key}
{/snippet}

<Story name="Key presses play" {template} />
<Story name="Key toggles loop" args={{ action: 'loop' }} {template} />
