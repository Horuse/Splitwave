<script lang="ts">
	import { platform } from '@tauri-apps/plugin-os';
	import { accelerator, formatAccelerator } from './_accelerator';

	interface Props {
		hotkey?: string;
		error?: string;
		onChange: (hotkey: string | undefined) => void;
		/** Lights the key chips. */
		held?: boolean;
	}
	let { hotkey, error = '', onChange, held = false }: Props = $props();

	let binding = $state(false);

	const CAP = 'flex h-[26px] min-w-7 items-center justify-center rounded-md border bg-neutral-100 px-2 text-xs font-medium group-hover:bg-neutral-200';
	const HELD = 'border-signal bg-signal/18 text-signal';

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
	title={binding ? 'Press a key. Esc cancels, Delete clears' : 'Click to bind a global shortcut'}
	class="group nodrag nopan flex cursor-pointer gap-1"
	onclick={() => (binding = !binding)}>
	{#if binding || caps.length === 0}
		<span class={[CAP, 'min-w-24 text-[10px]', binding ? 'border-primary text-primary' : 'border-neutral-400 text-neutral-900']}
			>{binding ? 'press a key' : 'bind key'}</span>
	{:else}
		{#each caps as cap, i (i)}
			<span class={[CAP, cap.modifier && 'text-[10px]', held ? HELD : ['border-neutral-400', cap.modifier ? 'text-neutral-1000' : 'text-neutral-1200']]}
				>{cap.label}</span>
		{/each}
	{/if}
</button>

{#if error}
	<p class="mt-1 w-40 text-[10px] break-words text-red-500">{error}</p>
{/if}

<svelte:window onkeydown={binding ? capture : undefined} />
