<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import AnimatedNumber from './animated-number.svelte';

	const { Story } = defineMeta({ title: 'Data/AnimatedNumber', component: AnimatedNumber, tags: ['autodocs'], args: { value: 9.7 } });
</script>

<script lang="ts">
	let value = $state(9.7);
	$effect(() => {
		const id = setInterval(() => (value = Math.round((5 + Math.random() * 10) * 10) / 10), 1500);
		return () => clearInterval(id);
	});
</script>

<Story name="Latency">
	{#snippet template()}
		<div class="bg-background p-6 text-3xl">
			<AnimatedNumber {value} suffix=" ms" format={{ maximumFractionDigits: 1 }} />
		</div>
	{/snippet}
</Story>

<Story name="Percent">
	{#snippet template()}
		<div class="bg-background p-6 text-3xl"><AnimatedNumber value={42} suffix="%" /></div>
	{/snippet}
</Story>
