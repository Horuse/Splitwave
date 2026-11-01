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
	<div
		class={[
			'flex w-40 items-center justify-center gap-2 rounded-lg border px-3 py-2 transition-colors',
			held ? 'border-yellow-500/60 bg-yellow-500/10' : 'border-neutral-400 bg-neutral-100'
		]}>
		<span
			class={[
				'flex size-6 items-center justify-center rounded-full transition-colors',
				held ? 'bg-yellow-500 text-white shadow-[0_0_8px_rgba(234,179,8,0.7)]' : 'bg-neutral-300 text-neutral-600'
			]}>
			<KeyCommand class="size-3.5" />
		</span>
		<span class={['text-sm font-medium', held ? 'text-yellow-600 dark:text-yellow-400' : 'text-neutral-1100']}>
			{held ? 'Held' : hotkey ? 'Idle' : 'No key'}
		</span>
	</div>

	<div class="mt-3">
		<HotkeyBinder {hotkey} error={shortcut.error} onChange={setHotkey} />
	</div>

	<div class="mt-2">
		<ControlPort id={CONTROL_OUT} side="source" label="out" high={held} />
	</div>
</Wrapper>
