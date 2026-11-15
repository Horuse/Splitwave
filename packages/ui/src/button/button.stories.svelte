<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import IconAdd from '~icons/fluent/add-24-regular';
	import Button from './button.svelte';

	const { Story } = defineMeta({
		title: 'Buttons/Button',
		component: Button,
		tags: ['autodocs'],
		argTypes: {
			tone: { control: 'select', options: ['neutral', 'accent', 'success', 'danger', 'warning'] },
			variant: { control: 'select', options: ['soft', 'solid', 'ghost'] },
			size: { control: 'select', options: ['xs', 'sm', 'md', 'lg'] },
			icon: { control: 'boolean' },
			active: { control: 'boolean' },
			disabled: { control: 'boolean' }
		},
		args: { tone: 'neutral', variant: 'soft', size: 'md', icon: false, active: false, disabled: false }
	});

	const tones = ['neutral', 'accent', 'success', 'danger', 'warning'] as const;
	const variants = ['soft', 'solid', 'ghost'] as const;
	const sizes = ['xs', 'sm', 'md', 'lg'] as const;
</script>

<Story name="Playground">
	{#snippet template(args)}
		<div class="p-6">
			<Button {...args}>Button</Button>
		</div>
	{/snippet}
</Story>

<Story name="Matrix">
	<div class="flex flex-col gap-3 p-6">
		{#each variants as variant (variant)}
			<div class="flex items-center gap-2">
				<span class="w-12 font-mono text-[11px] text-muted">{variant}</span>
				{#each tones as tone (tone)}
					<Button {tone} {variant}>{tone}</Button>
				{/each}
			</div>
		{/each}
	</div>
</Story>

<Story name="Sizes">
	<div class="flex items-center gap-2 p-6">
		{#each sizes as size (size)}
			<Button {size}>{size}</Button>
			<Button {size} icon aria-label="Add"><IconAdd /></Button>
		{/each}
	</div>
</Story>

<Story name="States">
	<div class="flex items-center gap-2 p-6">
		<Button>Default</Button>
		<Button active>Active</Button>
		<Button disabled>Disabled</Button>
		<Button href="/" tone="accent">Link</Button>
	</div>
</Story>
