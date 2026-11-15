<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import AnimatedTime from './animated-time.svelte';

	const { Story } = defineMeta({ title: 'Data/AnimatedTime', component: AnimatedTime, tags: ['autodocs'], args: { value: 3_725_000 } });
</script>

<script lang="ts">
	let ticking = $state(55_000);
	$effect(() => {
		const id = setInterval(() => (ticking += 1000), 1000);
		return () => clearInterval(id);
	});
</script>

<Story name="Static">
	{#snippet template(args)}
		<div class="bg-background p-6 text-3xl"><AnimatedTime {...args} /></div>
	{/snippet}
</Story>

<Story name="Ticking">
	{#snippet template()}
		<div class="bg-background p-6 text-3xl"><AnimatedTime value={ticking} /></div>
	{/snippet}
</Story>
