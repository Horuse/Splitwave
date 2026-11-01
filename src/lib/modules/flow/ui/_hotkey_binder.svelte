<script lang="ts">
	import { Dismiss } from '$lib/components/icons';
	import { accelerator, formatAccelerator } from './_accelerator';

	interface Props {
		hotkey?: string;
		error?: string;
		onChange: (hotkey: string | undefined) => void;
		/** Draws the binding as laptop keys that sink and backlight while held. */
		keycap?: boolean;
		held?: boolean;
	}
	let { hotkey, error = '', onChange, keycap = false, held = false }: Props = $props();

	let binding = $state(false);
	let legend = $derived(binding ? 'Press a key...' : hotkey ? formatAccelerator(hotkey) : 'Bind key');
	// One cap per key of the combo; a prompt is a single wide cap.
	let caps = $derived(binding || !hotkey ? [legend] : legend.split(' '));

	function capture(e: KeyboardEvent) {
		e.preventDefault();
		if (e.key === 'Escape') {
			binding = false;
			return;
		}
		const combo = accelerator(e);
		if (!combo) return;
		binding = false;
		onChange(combo);
	}
</script>

<div class={['nodrag nopan flex items-center gap-1', !keycap && 'w-40']}>
	{#if keycap}
		<button
			title={binding ? 'Press any key, Esc to cancel' : 'Click to bind a global shortcut'}
			class={['keys flex items-center gap-1', held && 'held', binding && 'binding']}
			onclick={() => (binding = !binding)}>
			{#each caps as cap, i (i)}
				<span class={['cap', cap.length > 2 && 'wide']}>{cap}</span>
			{/each}
		</button>
	{:else}
		<button
			title={binding ? 'Press any key, Esc to cancel' : 'Bind a global shortcut'}
			class={[
				'flex-1 truncate rounded-md border px-2 py-1 font-mono text-xs tabular-nums transition-colors',
				binding ? 'border-blue-500/60 bg-blue-500/10 text-blue-500' : 'border-neutral-400 bg-neutral-100 text-neutral-1100 hover:bg-neutral-200'
			]}
			onclick={() => (binding = !binding)}>
			{legend}
		</button>
	{/if}
	{#if hotkey && !binding}
		<button
			title="Clear shortcut"
			class="rounded-md border border-neutral-400 bg-neutral-100 p-1 text-neutral-900 hover:bg-neutral-200"
			onclick={() => onChange(undefined)}>
			<Dismiss class="size-3" />
		</button>
	{/if}
</div>

{#if error}
	<p class="mt-1 w-40 text-[10px] break-words text-red-500">{error}</p>
{/if}

<svelte:window onkeydown={binding ? capture : undefined} />

<style>
	/* Laptop key caps: near-black, flat, faint top light; held keys sink and
	   their legends take the signal colour like a keyboard backlight. */
	.cap {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		min-width: 1.75rem;
		height: 1.75rem;
		padding: 0 0.375rem;
		border-radius: 5px;
		background: linear-gradient(to bottom, #2b2b2d, #1c1c1e);
		color: #f5f5f7;
		font:
			500 11px -apple-system,
			BlinkMacSystemFont,
			system-ui,
			sans-serif;
		box-shadow:
			inset 0 1px 0 rgb(255 255 255 / 0.08),
			0 1px 0 rgb(0 0 0 / 0.5),
			0 2px 3px rgb(0 0 0 / 0.25);
		transition:
			transform 80ms,
			box-shadow 80ms,
			color 120ms,
			text-shadow 120ms;
	}
	.cap.wide {
		padding: 0 0.625rem;
	}
	.keys:hover .cap {
		background: linear-gradient(to bottom, #333336, #222224);
	}
	.keys.held .cap {
		transform: translateY(1px) scale(0.97);
		color: var(--color-signal);
		text-shadow: 0 0 6px var(--color-signal);
		box-shadow:
			inset 0 1px 0 rgb(255 255 255 / 0.04),
			0 0 0 rgb(0 0 0 / 0.5),
			0 0 10px color-mix(in oklch, var(--color-signal) 35%, transparent);
	}
	.keys.binding .cap {
		color: var(--color-blue-400, #60a5fa);
	}
</style>
