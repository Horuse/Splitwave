<script lang="ts">
	import { Combobox, ComboboxAction } from '@splitwave/ui';
	import Delete from '~icons/fluent/delete-16-filled';
	import Add from '~icons/fluent/add-24-filled';
	import type { Preset, PresetData } from '../../presets';
	import { getEnginePort, type PluginParam } from '../../engine';

	interface Props {
		pluginId: string;
		params: PluginParam[];
		onApply: (values: Record<string, number>) => void;
	}

	let { pluginId, params, onApply }: Props = $props();

	const store = getEnginePort().presets;

	let presets = $state<Preset<'plugin'>[]>([]);

	async function reload() {
		const all = (await store.list('plugin')) as Preset<'plugin'>[];
		presets = all.filter((p) => p.data.pluginId === pluginId);
	}
	$effect(() => {
		pluginId;
		reload();
	});

	// Selected while every stored value still matches the plugin, so a knob turn drops the label.
	function matches(p: Preset<'plugin'>): boolean {
		return params.length > 0 && params.every((x) => p.data.params[x.id] === undefined || Math.abs(p.data.params[x.id] - x.value) < 1e-6);
	}

	let active = $derived(presets.find(matches));
	let options = $derived(presets.map((p) => ({ value: p.id, label: p.name })));

	function apply(presetId: string | null) {
		const p = presets.find((x) => x.id === presetId);
		if (p) onApply(p.data.params);
	}

	async function save() {
		const name = await store.promptName(presets.map((p) => p.name));
		if (!name) return;
		const data: PresetData<'plugin'> = { pluginId, params: Object.fromEntries(params.map((p) => [p.id, p.value])) };
		await store.create('plugin', name, data);
		await reload();
	}

	async function remove() {
		if (!active) return;
		await store.remove(active.id);
		await reload();
	}
</script>

<div class="nodrag nopan">
	<Combobox {options} value={active?.id ?? null} placeholder="No preset" emptyHint="No presets saved yet" onChange={apply}>
		{#snippet footer(close)}
			<ComboboxAction
				label="Save current values"
				icon={Add}
				onclick={() => {
					close();
					save();
				}} />
			{#if active}
				<ComboboxAction
					label="Delete this preset"
					icon={Delete}
					onclick={() => {
						close();
						remove();
					}} />
			{/if}
		{/snippet}
	</Combobox>
</div>
