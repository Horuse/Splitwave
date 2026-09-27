<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { fn } from 'storybook/test';
	import { Combobox, ComboboxAction, RescanButton } from '$lib/modules/form/ui';
	import { Add } from '$lib/components/icons';
	import Controlled from '../harness/controlled.svelte';

	const DEVICES = [
		{ value: 'in-1', label: 'MacBook Pro Microphone' },
		{ value: 'in-2', label: 'Scarlett 2i2 USB' },
		{ value: 'in-3', label: 'Splitwave' },
		{ value: 'in-4', label: 'AirPods Pro' }
	];
	const PLUGINS = [
		{ value: 'p1', label: 'Pro-Q 3', subtitle: 'FabFilter', badge: 'clap' },
		{ value: 'p2', label: 'Valhalla Supermassive', subtitle: 'Valhalla DSP', badge: 'clap' },
		{ value: 'p3', label: 'AUDelay', subtitle: 'Apple', badge: 'au' }
	];
	const LONG = Array.from({ length: 40 }, (_, i) => ({ value: `o${i}`, label: `Option number ${i + 1}` }));
	const LISTS = { devices: DEVICES, plugins: PLUGINS, long: LONG, empty: [] };

	const { Story } = defineMeta({
		title: 'Forms/Combobox',
		argTypes: {
			options: { control: 'select', options: Object.keys(LISTS), mapping: LISTS },
			value: { control: 'text' },
			placeholder: { control: 'text' },
			emptyHint: { control: 'text' },
			size: { control: 'inline-radio', options: ['sm', 'md'] },
			footer: { control: 'inline-radio', options: ['none', 'rescan', 'action'] }
		},
		args: {
			options: 'devices',
			value: 'in-2',
			placeholder: '— Select —',
			emptyHint: 'No matches',
			size: 'md',
			footer: 'none',
			onChange: fn(),
			onOpen: fn()
		}
	});
</script>

{#snippet rescan()}
	<RescanButton onRescan={() => new Promise((r) => setTimeout(r, 800))} />
{/snippet}

{#snippet action(close: () => void)}
	<ComboboxAction label="Add virtual device" icon={Add} onclick={close} />
	<RescanButton onRescan={() => new Promise((r) => setTimeout(r, 800))} />
{/snippet}

{#snippet template({ footer, onChange, ...args }: Record<string, any>)}
	<div class="p-8">
		<div class={args.size === 'sm' ? 'w-56' : 'w-72'}>
			<Controlled component={Combobox} props={{ ...args, footer: footer === 'rescan' ? rescan : footer === 'action' ? action : undefined }} {onChange} />
		</div>
	</div>
{/snippet}

<Story name="Default" {template} />
<Story name="Empty value" args={{ value: null }} {template} />
<Story name="Compact" args={{ size: 'sm' }} {template} />
<Story name="With subtitles and badges" args={{ options: 'plugins', value: 'p1' }} {template} />
<Story name="Long list" args={{ options: 'long', value: 'o12' }} {template} />
<Story name="No options" args={{ options: 'empty', value: null, emptyHint: 'No devices found' }} {template} />
<Story name="Rescan footer" args={{ footer: 'rescan' }} {template} />
<Story name="Action footer" args={{ footer: 'action', size: 'sm' }} {template} />
