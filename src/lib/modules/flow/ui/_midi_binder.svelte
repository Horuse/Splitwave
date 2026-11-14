<script lang="ts">
	import type { MidiBinding } from '$lib/modules/pipeline/types';
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
	title={learning ? 'Press a note or move a control. Esc cancels, Delete clears' : 'Click to learn a MIDI control'}
	class={[
		'nodrag nopan flex h-[26px] min-w-24 cursor-pointer items-center justify-center rounded-md border px-2 text-[11px] font-medium',
		held
			? 'border-signal bg-signal/18 text-signal'
			: learning
				? 'border-primary bg-neutral-100 text-primary'
				: 'border-neutral-400 bg-neutral-100 text-neutral-1200 hover:bg-neutral-200'
	]}
	onclick={() => onLearn(!learning)}>
	{learning ? 'play a note' : binding ? formatMidi(binding) : 'learn MIDI'}
</button>
{#if binding && !learning}
	<p class="mt-1 w-40 truncate text-[10px] text-neutral-800" title={binding.device}>{binding.device}</p>
{/if}

<svelte:window onkeydown={learning ? capture : undefined} />
