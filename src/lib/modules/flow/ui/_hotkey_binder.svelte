<script lang="ts">
	import { Dismiss } from '$lib/components/icons';
	import { accelerator, formatAccelerator } from './_accelerator';

	interface Props {
		hotkey?: string;
		error?: string;
		onChange: (hotkey: string | undefined) => void;
	}
	let { hotkey, error = '', onChange }: Props = $props();

	let binding = $state(false);

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

<div class="nodrag nopan flex w-40 items-center gap-1">
	<button
		title={binding ? 'Press any key, Esc to cancel' : 'Bind a global shortcut'}
		class={[
			'flex-1 truncate rounded-md border px-2 py-1 font-mono text-xs tabular-nums transition-colors',
			binding ? 'border-blue-500/60 bg-blue-500/10 text-blue-500' : 'border-neutral-400 bg-neutral-100 text-neutral-1100 hover:bg-neutral-200'
		]}
		onclick={() => (binding = !binding)}>
		{binding ? 'Press a key...' : hotkey ? formatAccelerator(hotkey) : 'Bind key'}
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
