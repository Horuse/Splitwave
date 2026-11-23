<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import Callout from './callout.svelte';

	const { Story } = defineMeta({
		title: 'Feedback/Callout',
		component: Callout,
		tags: ['autodocs'],
		argTypes: { tone: { control: 'select', options: ['warning', 'info', 'success', 'danger'] } }
	});

	const tones = ['warning', 'info', 'success', 'danger'] as const;
</script>

<Story name="Tones">
	{#snippet template()}
		<div class="flex w-96 flex-col gap-3 p-6">
			{#each tones as tone (tone)}
				<Callout {tone}>This change takes effect on the running pipeline ({tone}).</Callout>
			{/each}
		</div>
	{/snippet}
</Story>
