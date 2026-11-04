<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import ControlCanvas from '../harness/control_canvas.svelte';
	import { CONTROL_OUT, POWER_HANDLE } from '$lib/modules/flow/utils';

	const { Story } = defineMeta({
		title: 'Nodes/Conditions/Power Input',
		argTypes: { on: { control: 'boolean', description: 'Toggle state driving the gain' } },
		args: { on: true }
	});
</script>

{#snippet template(a: Record<string, any>)}
	{#key a.on}
		<ControlCanvas from="toggle" fromData={{ on: a.on }} fromHandle={CONTROL_OUT} to="gain" toHandle={POWER_HANDLE} />
	{/key}
{/snippet}

<Story name="Toggle on runs the effect" {template} />
<Story name="Toggle off bypasses it" args={{ on: false }} {template} />
