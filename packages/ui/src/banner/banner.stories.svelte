<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { Button } from '../button';
	import { Card } from '../card';
	import { Separator } from '../separator';
	import Banner from './banner.svelte';

	const { Story } = defineMeta({ title: 'Brand/Banner', component: Banner, tags: ['autodocs'], parameters: { layout: 'fullscreen' } });
</script>

<Story name="Announcement">
	{#snippet template()}
		<div class="flex flex-col gap-6 bg-background p-8">
			<Banner class="flex flex-col items-center gap-3 px-8 py-14 text-center">
				<h2 class="text-2xl font-medium">Splitwave 2.0 is here</h2>
				<p class="max-w-md text-sm text-white/85">Real-time engine rework, adaptive buffering and new control nodes.</p>
				<Button class="mt-2 border-transparent bg-white text-black hover:bg-white/90">See changelog</Button>
			</Banner>
			<div class="grid grid-cols-3 gap-4">
				<Card class="p-5 text-sm">Plain card</Card>
				<Card dashed class="p-5 text-sm">Dashed card</Card>
				<Card class="flex flex-col gap-3 p-5 text-sm">
					Card with separator
					<Separator />
					<Button tone="brand">Activate</Button>
				</Card>
			</div>
		</div>
	{/snippet}
</Story>
