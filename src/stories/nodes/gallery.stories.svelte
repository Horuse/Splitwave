<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import Gallery from '../harness/gallery.svelte';
	import { compose, signalArgs, splitArgs } from '../harness/args';

	const { argTypes, args } = compose(signalArgs());

	const { Story } = defineMeta({ title: 'Nodes/Gallery', argTypes, args });
</script>

{#snippet template(a: Record<string, unknown>)}
	<Gallery signal={splitArgs(a).signal} />
{/snippet}

<Story name="All nodes" {template} />
<Story name="Silent" args={{ signal: 'off' }} {template} />
