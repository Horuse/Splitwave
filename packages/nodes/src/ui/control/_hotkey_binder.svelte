<script lang="ts">
	import { accelerator, formatAccelerator } from './_accelerator';
	import { getEnginePort } from '../../engine';

	const engine = getEnginePort();

	interface Props {
		hotkey?: string;
		error?: string;
		onChange: (hotkey: string | undefined) => void;
		/** Lights the key chips. */
		held?: boolean;
	}
	let { hotkey, error = '', onChange, held = false }: Props = $props();

	let binding = $state(false);

	const CAP = 'flex h-6 min-w-6 items-center justify-center rounded-md px-1.5 text-xs font-medium';

	// Modifiers are named the way this platform's keyboard prints them.
	const os = engine.os;
	const MODIFIERS: Record<string, string> = {
		Command: os === 'macos' ? 'Cmd' : os === 'windows' ? 'Win' : 'Super',
		Control: 'Ctrl',
		Alt: os === 'macos' ? 'Option' : 'Alt',
		Shift: 'Shift'
	};

	type Cap = { label: string; modifier: boolean };
	let caps = $derived.by((): Cap[] => {
		if (!hotkey) return [];
		return hotkey
			.split('+')
			.map((part) => (part in MODIFIERS ? { label: MODIFIERS[part], modifier: true } : { label: formatAccelerator(part), modifier: false }));
	});

	function capture(e: KeyboardEvent) {
		e.preventDefault();
		if (e.key === 'Escape') {
			binding = false;
			return;
		}
		if ((e.code === 'Backspace' || e.code === 'Delete') && !e.metaKey && !e.ctrlKey && !e.altKey) {
			binding = false;
			onChange(undefined);
			return;
		}
		const combo = accelerator(e);
		if (!combo) return;
		binding = false;
		onChange(combo);
	}
</script>

<button
	type="button"
	title={binding ? 'Press a key. Esc cancels, Delete clears' : 'Click to bind a global shortcut'}
	class={[
		'nodrag nopan flex min-h-14 w-full cursor-pointer flex-col items-center justify-center gap-1 rounded-lg border bg-(--field-bg,var(--lv-1)) px-2 py-2',
		binding ? 'border-dashed border-signal/60' : 'border-theme/5 hover:border-(--lv-4)'
	]}
	onclick={() => (binding = !binding)}>
	{#if binding}
		<span class="text-xs text-signal">Press a key...</span>
		<span class="text-[9px] text-neutral-600">Esc cancels · Delete clears</span>
	{:else if caps.length === 0}
		<span class="text-xs text-neutral-700">Click to bind a key</span>
	{:else}
		<span class="flex flex-wrap items-center justify-center gap-1">
			{#each caps as cap, i (i)}
				<span class={[CAP, cap.modifier && 'text-[10px]', held ? 'bg-signal text-neutral-50' : 'bg-(--lv-2) text-neutral-1100']}>{cap.label}</span>
			{/each}
		</span>
	{/if}
</button>

{#if error}
	<p class="mt-1 text-[10px] break-words text-red-500">{error}</p>
{/if}

<svelte:window onkeydown={binding ? capture : undefined} />
