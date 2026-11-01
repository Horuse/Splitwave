<script lang="ts">
	import { Dismiss } from '$lib/components/icons';
	import { accelerator, formatAccelerator } from './_accelerator';

	interface Props {
		hotkey?: string;
		error?: string;
		onChange: (hotkey: string | undefined) => void;
		/** Draws the binding as a physical key that sinks and glows while held. */
		keycap?: boolean;
		held?: boolean;
	}
	let { hotkey, error = '', onChange, keycap = false, held = false }: Props = $props();

	let binding = $state(false);
	let legend = $derived(binding ? 'Press a key...' : hotkey ? formatAccelerator(hotkey) : 'Bind key');

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

<div class={['nodrag nopan flex items-center gap-1', keycap ? 'w-32' : 'w-40']}>
	<button
		title={binding ? 'Press any key, Esc to cancel' : 'Bind a global shortcut'}
		class={[
			'flex-1 truncate font-mono tabular-nums transition-all',
			keycap ? ['keycap h-7 rounded-md border px-2 text-xs', held && 'held', binding && 'binding'] : 'rounded-md border px-2 py-1 text-xs',
			!keycap &&
				(binding ? 'border-blue-500/60 bg-blue-500/10 text-blue-500' : 'border-neutral-400 bg-neutral-100 text-neutral-1100 hover:bg-neutral-200')
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

{#if error}
	<p class="mt-1 w-40 text-[10px] break-words text-red-500">{error}</p>
{/if}

<svelte:window onkeydown={binding ? capture : undefined} />

<style>
	/* A key cap: its travel is the bottom edge, which collapses as it is pressed. */
	.keycap {
		border-color: var(--color-neutral-500);
		background: linear-gradient(to bottom, var(--color-neutral-50), var(--color-neutral-100));
		color: var(--color-neutral-1100);
		box-shadow: 0 3px 0 var(--color-neutral-500);
		transform: translateY(0);
	}
	.keycap:hover {
		background: var(--color-neutral-50);
	}
	.keycap.held {
		border-color: var(--color-signal);
		color: var(--color-signal);
		box-shadow:
			0 1px 0 var(--color-neutral-500),
			0 0 12px color-mix(in oklch, var(--color-signal) 45%, transparent);
		transform: translateY(2px);
	}
	.keycap.binding {
		border-color: var(--color-blue-500);
		color: var(--color-blue-500);
	}
</style>
