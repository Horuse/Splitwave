<script lang="ts" generics="K extends PresetKind">
	import { Combobox, ComboboxAction } from '$lib/modules/form/ui';
	import { modalManager } from '$lib/modules/overlay/modal';
	import { Delete, Add } from '$lib/components/icons';
	import type { NodeDataMap } from '$lib/modules/pipeline/types';
	import { methods } from '../methods';
	import type { Preset, PresetData, PresetKind } from '../types';
	import SavePresetModal from './save_preset_modal.svelte';

	interface Props {
		kind: K;
		data: NodeDataMap[K];
		onApply: (data: PresetData<K>) => void;
	}

	let { kind, data, onApply }: Props = $props();

	let presets = $state<Preset[]>([]);
	let selectedId = $state<string | null>(null);

	async function reload() {
		presets = await methods.list(kind);
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
		const name = await modalManager.open<string>('Save preset', SavePresetModal, {
			size: 'sm',
			taken: presets.filter((p) => !p.builtin).map((p) => p.name)
		});
		if (!name) return;
		const created = await methods.create(kind, name, strip(data));
		await reload();
		selectedId = created.id;
	}

	async function remove() {
		if (!active || active.builtin) return;
		await methods.remove(active.id);
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
