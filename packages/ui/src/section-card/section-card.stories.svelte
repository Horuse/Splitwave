<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import IconPaint from '~icons/fluent/paint-brush-20-regular';
	import { Chip } from '../chip';
	import { Separator } from '../separator';
	import { Toggle } from '../toggle';
	import SectionCard from './section-card.svelte';

	const { Story } = defineMeta({ title: 'Layout/Section Card', component: SectionCard, tags: ['autodocs'] });
</script>

<script lang="ts">
	let theme = $state('dark');
	let animate = $state(true);
</script>

<Story name="Settings group">
	{#snippet template()}
		<div class="max-w-xl bg-background p-8 text-theme">
			<SectionCard title="Appearance" description="How Splitwave looks." icon={IconPaint}>
				<div class="flex gap-2">
					{#each ['light', 'dark', 'system'] as t (t)}
						<Chip selected={theme === t} onclick={() => (theme = t)}>{t}</Chip>
					{/each}
				</div>
				<Separator />
				<Toggle checked={animate} label="Animate cables" hint="Marching dashes along every connection." onChange={(v) => (animate = v)} />
			</SectionCard>
		</div>
	{/snippet}
</Story>
