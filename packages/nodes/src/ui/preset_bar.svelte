<script lang="ts" generics="K extends PresetKind">
	import { Combobox, ComboboxAction } from '@splitwave/ui';
	import Delete from '~icons/fluent/delete-16-filled';
	import Add from '~icons/fluent/add-24-filled';
	import type { NodeDataMap } from '@splitwave/nodes';
	import type { Preset, PresetData, PresetKind } from '../presets';
	import { getEnginePort } from '../engine';

	interface Props {
		kind: K;
		data: NodeDataMap[K];
		onApply: (data: PresetData<K>) => void;
	}

	let { kind, data, onApply }: Props = $props();

	const presetStore = getEnginePort().presets;

	let presets = $state<Preset[]>([]);
	let selectedId = $state<string | null>(null);

	async function reload() {
		presets = await presetStore.list(kind);
	}
	$effect(() => {
		kind;
		reload();
	});

	function strip(d: NodeDataMap[K]): PresetData<K> {
		const { bypassed: _bypassed, ...rest } = d as Record<string, unknown>;
		return rest as PresetData<K>;
	}

	// A preset stays selected only while every one of its fields still matches;
	// touching any slider drops the label rather than lying about what is loaded.
	function matches(p: Preset): boolean {
		const current = data as Record<string, unknown>;
		return Object.entries(p.data as Record<string, unknown>).every(([k, v]) => JSON.stringify(current[k]) === JSON.stringify(v));
	}

	let active = $derived(presets.find((p) => p.id === selectedId && matches(p)) ?? presets.find(matches));
	let options = $derived(presets.map((p) => ({ value: p.id, label: p.builtin ? p.name : `${p.name} *` })));

	function apply(id: string | null) {
		const p = presets.find((x) => x.id === id);
		if (!p) return;
		selectedId = p.id;
		onApply(p.data as PresetData<K>);
	}

	async function save() {
		const name = await presetStore.promptName(presets.filter((p) => !p.builtin).map((p) => p.name));
		if (!name) return;
		const created = await presetStore.create(kind, name, strip(data));
		await reload();
		selectedId = created.id;
	}

	async function remove() {
		if (!active || active.builtin) return;
		await presetStore.remove(active.id);
		selectedId = null;
		await reload();
	}
</script>

<div class="nodrag nopan">
	<Combobox {options} value={active?.id ?? null} placeholder="— Preset —" emptyHint="No presets" onChange={apply}>
		{#snippet footer(close)}
			<ComboboxAction
				label="Save current settings"
				icon={Add}
				onclick={() => {
					close();
					save();
				}} />
			{#if active && !active.builtin}
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
