<script lang="ts">
	import { Dismiss } from '$lib/components/icons';
	import { accelerator, formatAccelerator } from './_accelerator';

	interface Props {
		hotkey?: string;
		error?: string;
		onChange: (hotkey: string | undefined) => void;
		/** Draws the binding as laptop keys set in a keyboard well; they sink and
		 * backlight while held. Delete clears it while binding. */
		keycap?: boolean;
		held?: boolean;
	}
	let { hotkey, error = '', onChange, keycap = false, held = false }: Props = $props();

	let binding = $state(false);
	let legend = $derived(binding ? 'Press a key...' : hotkey ? formatAccelerator(hotkey) : 'Bind key');

	// Modifier caps carry a glyph and a word, the way a Mac keyboard prints them.
	const MODIFIERS: Record<string, { glyph: string; word: string }> = {
		Command: { glyph: '⌘', word: 'command' },
		Control: { glyph: '⌃', word: 'control' },
		Alt: { glyph: '⌥', word: 'option' },
		Shift: { glyph: '⇧', word: 'shift' }
	};

	type Cap = { glyph: string; word?: string };
	let caps = $derived.by((): Cap[] => {
		if (!hotkey) return [];
		return hotkey.split('+').map((part) => MODIFIERS[part] ?? { glyph: formatAccelerator(part) });
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
				{#if cap.word}
					<span class="cap mod"><span class="glyph">{cap.glyph}</span><span class="word">{cap.word}</span></span>
				{:else}
					<span class="cap">{cap.glyph}</span>
				{/if}
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
	/* The keyboard well: a recessed slot of the laptop deck the keys sit in. */
	.well {
		display: flex;
		gap: 5px;
		padding: 5px;
		border-radius: 9px;
		background: #0e0e10;
		box-shadow:
			inset 0 1px 3px rgb(0 0 0 / 0.8),
			inset 0 0 0 1px rgb(255 255 255 / 0.04),
			0 1px 0 rgb(255 255 255 / 0.06);
		transition: box-shadow 160ms;
		cursor: pointer;
	}
	.well.held {
		box-shadow:
			inset 0 1px 3px rgb(0 0 0 / 0.8),
			inset 0 0 14px color-mix(in oklch, var(--color-signal) 30%, transparent),
			0 1px 0 rgb(255 255 255 / 0.06);
	}
	.well.binding {
		box-shadow:
			inset 0 1px 3px rgb(0 0 0 / 0.8),
			inset 0 0 0 1px rgb(96 165 250 / 0.6);
	}

	.cap {
		position: relative;
		display: flex;
		align-items: center;
		justify-content: center;
		min-width: 34px;
		height: 32px;
		padding: 0 8px;
		border-radius: 5px;
		background: linear-gradient(to bottom, #2a2a2c 0%, #1f1f21 100%);
		color: #f2f2f4;
		font:
			400 13px -apple-system,
			BlinkMacSystemFont,
			'SF Pro Text',
			system-ui,
			sans-serif;
		box-shadow:
			inset 0 1px 0 rgb(255 255 255 / 0.07),
			inset 0 -1px 0 rgb(0 0 0 / 0.4),
			0 1px 1px rgb(0 0 0 / 0.6);
		transition:
			transform 70ms ease-out,
			color 120ms,
			text-shadow 120ms,
			box-shadow 120ms;
	}
	/* Mac modifier caps: glyph top right, word bottom left. */
	.cap.mod {
		min-width: 52px;
		padding: 0;
	}
	.mod .glyph {
		position: absolute;
		top: 3px;
		right: 5px;
		font-size: 10px;
	}
	.mod .word {
		position: absolute;
		bottom: 3px;
		left: 5px;
		font-size: 8px;
		letter-spacing: 0.01em;
	}
	.cap.prompt {
		min-width: 96px;
		font-size: 10px;
		color: #8e8e93;
	}
	.well:hover .cap {
		background: linear-gradient(to bottom, #303033 0%, #232325 100%);
	}
	.well.held .cap {
		transform: translateY(1px);
		color: var(--color-signal);
		text-shadow: 0 0 6px var(--color-signal);
		box-shadow:
			inset 0 1px 0 rgb(255 255 255 / 0.03),
			inset 0 -1px 0 rgb(0 0 0 / 0.4),
			0 0 8px color-mix(in oklch, var(--color-signal) 40%, transparent);
	}
	.well.binding .cap.prompt {
		color: #60a5fa;
	}
	@media (prefers-reduced-motion: reduce) {
		.well.held .cap {
			transform: none;
		}
	}
</style>
