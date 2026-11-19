<script lang="ts">
	import { Segmented } from '@splitwave/ui';
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { KeyInputNodeData, KeyInputSource } from '@splitwave/nodes';
	import Wrapper from '../node.svelte';
	import KeyCommand from '~icons/fluent/key-command-24-regular';
	import HotkeyBinder from './_hotkey_binder.svelte';
	import MidiBinder from './_midi_binder.svelte';
	import { useGlobalHotkey } from './_hotkey.svelte';
	import { useMidiControl } from './_midi.svelte';
	import ControlSocket from '../_control_socket.svelte';
	import { CONTROL_OUT } from '../utils';
	import { controlSignals } from '../state/control.svelte';

	type KeyInputNodeType = Node<KeyInputNodeData, 'keyInput'>;
	let { id, data }: NodeProps<KeyInputNodeType> = $props();

	const flow = useSvelteFlow();

	const SOURCES: { label: string; value: KeyInputSource }[] = [
		{ label: 'Keyboard', value: 'keyboard' },
		{ label: 'MIDI', value: 'midi' }
	];

	let held = $state(false);
	let learning = $state(false);
	let source = $derived(data.source ?? 'keyboard');
	let hotkey = $derived(source === 'keyboard' ? data.hotkey : undefined);

	const shortcut = useGlobalHotkey(
		() => hotkey,
		(state) => (held = state === 'Pressed')
	);

	useMidiControl(
		() => (source === 'midi' ? data.midi : undefined),
		() => learning,
		(midi) => set({ midi }),
		(on) => (held = on)
	);

	$effect(() => {
		controlSignals.emit(id, CONTROL_OUT, held);
		return () => controlSignals.clear(id, CONTROL_OUT);
	});

	function set(patch: Partial<KeyInputNodeData>) {
		held = false;
		learning = false;
		flow.updateNodeData(id, patch);
	}
</script>

<Wrapper label="Key Input" icon={KeyCommand} accent="control">
	<div class="flex flex-col gap-2 pb-1">
		<Segmented class="nodrag nopan" options={SOURCES} value={source} onSelect={(next) => set({ source: next })} />
		<div class="relative flex flex-col">
			{#if source === 'keyboard'}
				<HotkeyBinder {hotkey} error={shortcut.error} onChange={(next) => set({ hotkey: next })} {held} />
			{:else}
				<MidiBinder binding={data.midi} {learning} onLearn={(next) => (learning = next)} onClear={() => set({ midi: undefined })} {held} />
			{/if}
			<ControlSocket id={CONTROL_OUT} side="source" high={held} />
		</div>
	</div>
</Wrapper>
