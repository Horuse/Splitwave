<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { fn } from 'storybook/test';
	import HotkeyBinder from '$lib/modules/flow/ui/_hotkey_binder.svelte';
	import Controlled from '../harness/controlled.svelte';

	const { Story } = defineMeta({
		title: 'Forms/Hotkey Binder',
		argTypes: {
			hotkey: { control: 'select', options: [undefined, 'F13', 'CommandOrControl+Shift+M', 'Control+Alt+Shift+KeyT'] },
			error: { control: 'text' }
		},
		args: { hotkey: undefined, error: '', onChange: fn() }
	});
</script>

{#snippet template({ onChange, ...args }: Record<string, any>)}
	<div class="p-8">
		<Controlled component={HotkeyBinder} props={args} valueKey="hotkey" {onChange} />
	</div>
{/snippet}

<Story name="Unbound" {template} />
<Story name="Bound" args={{ hotkey: 'F13' }} {template} />
<Story name="Long combo" args={{ hotkey: 'Control+Alt+Shift+KeyT' }} {template} />
<Story name="Register failed" args={{ hotkey: 'F13', error: 'HotKey already registered' }} {template} />
