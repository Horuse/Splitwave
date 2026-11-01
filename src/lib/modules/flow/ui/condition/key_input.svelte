<script lang="ts">
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { KeyInputNodeData } from '$lib/modules/pipeline/types';
	import Wrapper from '../node.svelte';
	import { KeyCommand } from '$lib/components/icons';
	import HotkeyBinder from '../_hotkey_binder.svelte';
	import { useGlobalHotkey } from '../_hotkey.svelte';
	import ControlSocket from '../_control_socket.svelte';
	import { CONTROL_OUT } from '$lib/modules/flow/utils';
	import { controlSignals } from '../../control.svelte';

	type KeyInputNodeType = Node<KeyInputNodeData, 'keyInput'>;
	let { id, data }: NodeProps<KeyInputNodeType> = $props();

	const flow = useSvelteFlow();

	let held = $state(false);
	let hotkey = $derived(data.hotkey);

	const shortcut = useGlobalHotkey(
		() => hotkey,
		(state) => (held = state === 'Pressed')
	);

	$effect(() => {
		controlSignals.emit(id, CONTROL_OUT, held);
		return () => controlSignals.clear(id, CONTROL_OUT);
	});

	function setHotkey(next: string | undefined) {
		held = false;
		flow.updateNodeData(id, { hotkey: next });
	}
</script>

<Wrapper label="Key Input" icon={KeyCommand} accent="condition">
	<div class="relative flex items-center pb-1">
		<HotkeyBinder {hotkey} error={shortcut.error} onChange={setHotkey} keycap {held} />
		<ControlSocket id={CONTROL_OUT} side="source" high={held} />
	</div>
</Wrapper>
