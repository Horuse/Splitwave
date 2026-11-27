<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { ElapsedTime } from '$lib/modules/audio/ui';

	const { Story } = defineMeta({ title: 'Components/Elapsed Time', component: ElapsedTime });
</script>

{#snippet template(args: Record<string, any>)}
	<div class="p-8 text-sm text-brand tabular-nums"><ElapsedTime since={args.since} /></div>
{/snippet}

<Story name="Stopped" args={{ since: null }} {template} />
<Story name="Seconds" args={{ since: Date.now() - 9_000 }} {template} />
<Story name="Hours" args={{ since: Date.now() - 7_384_000 }} {template} />
