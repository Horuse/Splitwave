<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { PasswordInput } from '$lib/modules/form/ui';

	const { Story } = defineMeta({
		title: 'Forms/Password Input',
		argTypes: { value: { control: 'text' }, placeholder: { control: 'text' } },
		args: { value: '', placeholder: 'Password' }
	});
</script>

{#snippet template(args: Record<string, any>)}
	<div class="p-8">
		<div class="w-56">
			<PasswordInput value={args.value} placeholder={args.placeholder} />
		</div>
	</div>
{/snippet}

<Story name="Empty" {template} />
<Story name="Filled" args={{ value: 'hunter2' }} {template} />
<Story name="Custom placeholder" args={{ placeholder: 'Room password (optional)' }} {template} />
