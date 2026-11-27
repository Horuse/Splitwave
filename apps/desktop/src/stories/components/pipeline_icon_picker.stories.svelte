<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { Modal } from '@splitwave/ui';
	import IconPickerModal from '$lib/modules/pipeline/ui/icon_picker_modal.svelte';

	const { Story } = defineMeta({
		title: 'Components/Pipeline Icon Picker',
		component: IconPickerModal,
		parameters: { layout: 'fullscreen' },
		argTypes: {
			current: { control: 'text' },
			initialQuery: { control: 'text' }
		},
		args: { modalId: 'story', current: undefined, initialQuery: '' }
	});
</script>

{#snippet template(args: Record<string, any>)}
	<div class="h-[40rem]">
		<Modal title="Pipeline icon" size="lg" onClose={() => {}}>
			{#key `${args.current}/${args.initialQuery}`}
				<IconPickerModal {...args as any} />
			{/key}
		</Modal>
	</div>
{/snippet}

<Story name="All icons" {template} />
<Story name="Selected" args={{ current: 'headphones-20-filled', initialQuery: 'headphones' }} {template} />
<Story name="Search results" args={{ initialQuery: 'mic' }} {template} />
<Story name="No results" args={{ initialQuery: 'zzzz' }} {template} />
