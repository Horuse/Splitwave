<script lang="ts">
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { KeyInputNodeData } from '$lib/modules/pipeline/types';
	import Wrapper from '../node.svelte';
	import { KeyCommand } from '$lib/components/icons';
	import HotkeyBinder from '../_hotkey_binder.svelte';
	import { useGlobalHotkey } from '../_hotkey.svelte';
	import ControlPort from './_control_port.svelte';
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
	<HotkeyBinder {hotkey} error={shortcut.error} onChange={setHotkey} />
	<div class="mt-2">
		<ControlPort id={CONTROL_OUT} side="source" label={held ? 'held' : 'released'} high={held} />
	</div>
</Wrapper>
