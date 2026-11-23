<script lang="ts">
	import type { MidiBinding } from '@splitwave/nodes';
	import { formatMidi } from './_midi.svelte';

	interface Props {
		binding?: MidiBinding;
		learning: boolean;
		onLearn: (learning: boolean) => void;
		onClear: () => void;
		/** Lights the chip. */
		held?: boolean;
	}
	let { binding, learning, onLearn, onClear, held = false }: Props = $props();

	function capture(e: KeyboardEvent) {
		if (e.key === 'Escape') {
			e.preventDefault();
			onLearn(false);
		} else if (e.code === 'Backspace' || e.code === 'Delete') {
			e.preventDefault();
			onLearn(false);
			onClear();
		}
	}
</script>

<button
	type="button"
	title={learning ? 'Press a note or move a control. Esc cancels, Delete clears' : 'Click to learn a MIDI control'}
	class={[
		'nodrag nopan flex min-h-14 w-full cursor-pointer flex-col items-center justify-center gap-1 rounded-lg border bg-(--field-bg,var(--lv-1)) px-2 py-2',
		learning ? 'border-dashed border-signal/60' : 'border-theme/5 hover:border-(--lv-4)'
	]}
	onclick={() => onLearn(!learning)}>
	{#if learning}
		<span class="text-xs text-signal">Play a note or move a control...</span>
		<span class="text-[9px] text-neutral-600">Esc cancels · Delete clears</span>
	{:else if binding}
		<span
			class={[
				'flex h-6 items-center rounded-md px-2 font-mono text-xs font-medium',
				held ? 'bg-signal text-neutral-50' : 'bg-(--lv-2) text-neutral-1100'
			]}>
			{formatMidi(binding)}
		</span>
		<span class="max-w-full truncate text-[9px] text-neutral-600" title={binding.device}>{binding.device}</span>
	{:else}
		<span class="text-xs text-neutral-700">Click to learn MIDI</span>
	{/if}
</button>

<svelte:window onkeydown={learning ? capture : undefined} />
