<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import CopyButton from '$lib/components/copy_button.svelte';

	const { Story } = defineMeta({
		title: 'Components/Copy Button',
		component: CopyButton,
		argTypes: { text: { control: 'text' }, label: { control: 'text' }, copiedLabel: { control: 'text' } },
		args: { text: 'K7Q2XF', label: 'Copy', copiedLabel: 'Copied!' }
	});
</script>

{#snippet template(args: Record<string, any>)}
	<div class="p-8"><CopyButton {...args} text={args.text} /></div>
{/snippet}

<Story name="Default" {template} />
<Story name="Room code" args={{ label: 'Copy room code' }} {template} />
