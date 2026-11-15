<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { Button } from '../../button';
	import Modal from './modal.svelte';

	const { Story } = defineMeta({
		title: 'Overlay/Modal',
		component: Modal,
		tags: ['autodocs'],
		parameters: { layout: 'fullscreen' },
		argTypes: { size: { control: 'select', options: ['sm', 'md', 'lg', 'xl'] }, canClose: { control: 'boolean' } },
		args: { title: 'Save preset', description: 'Available in every pipeline.', size: 'md', canClose: true }
	});
</script>

<Story name="Default">
	{#snippet template(args)}
		<div class="h-[28rem]">
			<Modal {...args} onClose={() => {}}>
				<div class="text-muted px-5 py-4 text-xs">Modal body content.</div>
				{#snippet footer()}
					<Button>Cancel</Button>
					<Button tone="success">Save</Button>
				{/snippet}
			</Modal>
		</div>
	{/snippet}
</Story>
