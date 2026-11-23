<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import IconAdd from '~icons/fluent/add-16-filled';
	import Combobox from './combobox.svelte';
	import ComboboxAction from './combobox-action.svelte';
	import RescanButton from './rescan-button.svelte';

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

	const { Story } = defineMeta({
		title: 'Form/Combobox',
		component: Combobox,
		tags: ['autodocs'],
		argTypes: { size: { control: 'inline-radio', options: ['sm', 'md'] } }
	});
</script>

<script lang="ts">
	let device = $state<string | null>('in-2');
	let plugin = $state<string | null>('p1');
	let long = $state<string | null>('o12');
	const wait = () => new Promise<void>((r) => setTimeout(r, 800));
</script>

{#snippet rescan()}
	<RescanButton onRescan={wait} />
{/snippet}

{#snippet action(close: () => void)}
	<ComboboxAction label="Add virtual device" icon={IconAdd} onclick={close} />
	<RescanButton onRescan={wait} />
{/snippet}

<Story name="Default">
	{#snippet template()}
		<div class="w-72 p-8"><Combobox options={DEVICES} value={device} onChange={(v) => (device = v)} /></div>
	{/snippet}
</Story>

<Story name="Compact">
	{#snippet template()}
		<div class="w-56 p-8"><Combobox size="sm" options={DEVICES} value={device} onChange={(v) => (device = v)} /></div>
	{/snippet}
</Story>

<Story name="Subtitles and badges">
	{#snippet template()}
		<div class="w-72 p-8"><Combobox options={PLUGINS} value={plugin} onChange={(v) => (plugin = v)} /></div>
	{/snippet}
</Story>

<Story name="Long list">
	{#snippet template()}
		<div class="w-72 p-8"><Combobox options={LONG} value={long} onChange={(v) => (long = v)} /></div>
	{/snippet}
</Story>

<Story name="No options">
	{#snippet template()}
		<div class="w-72 p-8"><Combobox options={[]} value={null} emptyHint="No devices found" onChange={() => {}} /></div>
	{/snippet}
</Story>

<Story name="Footers">
	{#snippet template()}
		<div class="flex w-72 flex-col gap-16 p-8">
			<Combobox options={DEVICES} value={device} onChange={(v) => (device = v)} footer={rescan} />
			<Combobox size="sm" options={DEVICES} value={device} onChange={(v) => (device = v)} footer={action} />
		</div>
	{/snippet}
</Story>
