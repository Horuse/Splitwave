<script lang="ts">
	import { platform } from '@tauri-apps/plugin-os';
	import { Dismiss } from '$lib/components/icons';
	import { accelerator, formatAccelerator } from './_accelerator';

	interface Props {
		hotkey?: string;
		error?: string;
		onChange: (hotkey: string | undefined) => void;
		/** Draws the binding as one chip per key, lit while held. Delete clears
		 * it while binding. */
		keycap?: boolean;
		held?: boolean;
	}
	let { hotkey, error = '', onChange, keycap = false, held = false }: Props = $props();

	let binding = $state(false);
	let legend = $derived(binding ? 'Press a key...' : hotkey ? formatAccelerator(hotkey) : 'Bind key');

	// Modifiers are named the way this platform's keyboard prints them.
	const os = platform();
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
		if (keycap && (e.code === 'Backspace' || e.code === 'Delete') && !e.metaKey && !e.ctrlKey && !e.altKey) {
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

{#if keycap}
	<button
		title={binding ? 'Press a key. Esc cancels, Delete clears' : 'Click to bind a global shortcut'}
		class={['well nodrag nopan', held && 'held', binding && 'binding']}
		onclick={() => (binding = !binding)}>
		{#if binding || caps.length === 0}
			<span class="cap prompt">{binding ? 'press a key' : 'bind key'}</span>
		{:else}
			{#each caps as cap, i (i)}
				<span class={['cap', cap.modifier && 'mod']}>{cap.label}</span>
			{/each}
		{/if}
	</button>
{:else}
	<div class="nodrag nopan flex w-40 items-center gap-1">
		<button
			title={binding ? 'Press any key, Esc to cancel' : 'Bind a global shortcut'}
			class={[
				'flex-1 truncate rounded-md border px-2 py-1 font-mono text-xs tabular-nums transition-colors',
				binding ? 'border-blue-500/60 bg-blue-500/10 text-blue-500' : 'border-neutral-400 bg-neutral-100 text-neutral-1100 hover:bg-neutral-200'
			]}
			onclick={() => (binding = !binding)}>
			{legend}
		</button>
		{#if hotkey && !binding}
			<button
				title="Clear shortcut"
				class="rounded-md border border-neutral-400 bg-neutral-100 p-1 text-neutral-900 hover:bg-neutral-200"
				onclick={() => onChange(undefined)}>
				<Dismiss class="size-3" />
			</button>
		{/if}
	</div>
{/if}

{#if error}
	<p class="mt-1 w-40 text-[10px] break-words text-red-500">{error}</p>
{/if}

<svelte:window onkeydown={binding ? capture : undefined} />

<style>
	.well {
		display: flex;
		gap: 4px;
		cursor: pointer;
	}
	.cap {
		display: flex;
		align-items: center;
		justify-content: center;
		min-width: 28px;
		height: 26px;
		padding: 0 8px;
		border-radius: 6px;
		border: 1px solid var(--color-neutral-400);
		background: var(--color-neutral-100);
		color: var(--color-neutral-1200);
		font:
			500 12px system-ui,
			sans-serif;
		transition:
			background-color 120ms,
			border-color 120ms,
			color 120ms;
	}
	.cap.mod {
		font-size: 10px;
		color: var(--color-neutral-1000);
	}
	.cap.prompt {
		min-width: 96px;
		font-size: 10px;
		color: var(--color-neutral-900);
	}
	.well:hover .cap {
		background: var(--color-neutral-200);
	}
	.well.held .cap {
		border-color: var(--color-signal);
		background: color-mix(in oklch, var(--color-signal) 18%, var(--color-neutral-100));
		color: var(--color-signal);
	}
	.well.binding .cap.prompt {
		border-color: var(--color-primary);
		color: var(--color-primary);
	}
</style>
