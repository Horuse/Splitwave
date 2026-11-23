<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { ClassValue } from 'svelte/elements';
	import { smoothRadius } from '../attachments';

	type Props = {
		selected?: boolean;
		mono?: boolean;
		disabled?: boolean;
		title?: string;
		class?: ClassValue;
		onclick?: () => void;
		children?: Snippet;
	};

	let { selected = false, mono = false, disabled = false, title, class: className, onclick, children }: Props = $props();
</script>

<button
	type="button"
	class={['chip', mono && 'chip-mono', className]}
	data-selected={selected}
	aria-pressed={selected}
	{disabled}
	{title}
	{onclick}
	{@attach smoothRadius({ variable: '--r', maxRatio: 0.35 })}>
	{@render children?.()}
</button>
