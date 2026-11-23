<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import CableCanvas from '../harness/cable_canvas.svelte';

	const { Story } = defineMeta({
		title: 'Components/Cables',
		argTypes: {
			audio: { control: { type: 'range', min: 0, max: 4, step: 1 } },
			control: { control: { type: 'range', min: 0, max: 3, step: 1 } }
		},
		args: { audio: 2, control: 1 }
	});
</script>

{#snippet template(args: Record<string, any>)}
	<CableCanvas audio={args.audio} control={args.control} />
{/snippet}

<Story name="Audio and control" {template} />
<Story name="Audio only" args={{ audio: 2, control: 0 }} {template} />
<Story name="Control only" args={{ audio: 0, control: 2 }} {template} />
