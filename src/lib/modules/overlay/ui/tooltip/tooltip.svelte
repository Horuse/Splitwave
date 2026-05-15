<script lang="ts">
	import type { Snippet } from 'svelte';
	import { fade } from 'svelte/transition';
	import { createFloatingActions } from 'svelte-floating-ui';
	import { flip, offset, shift } from 'svelte-floating-ui/dom';
	import { portal } from 'svelte-portal';
	import type { Placement } from '../../types';

	interface Props {
		text?: string;
		content?: Snippet;
		placement?: Placement;
		offsetPx?: number;
		delay?: number;
		children: Snippet;
	}

	let {
		text,
		content,
		placement = 'top',
		offsetPx = 6,
		delay = 200,
		children
	}: Props = $props();

	let open = $state(false);
	let openTimer: ReturnType<typeof setTimeout> | undefined;

	const [floatingRef, floatingContent] = createFloatingActions({
		strategy: 'fixed',
		placement,
		middleware: [offset(offsetPx), flip(), shift({ padding: 6 })]
	});

	function show() {
		openTimer = setTimeout(() => (open = true), delay);
	}
	function hide() {
		if (openTimer !== undefined) {
			clearTimeout(openTimer);
			openTimer = undefined;
		}
		open = false;
	}
</script>

<div
	use:floatingRef
	onmouseenter={show}
	onmouseleave={hide}
	onfocusin={show}
	onfocusout={hide}
	class="inline-flex"
>
	{@render children()}
</div>

{#if open}
	<div
		use:portal={'#overlays'}
		use:floatingContent
		transition:fade={{ duration: 100 }}
		class="z-[200] pointer-events-none rounded-md border border-neutral-400 bg-neutral-1100 px-2 py-1 font-mono text-[10px] text-neutral-100 shadow-md"
		role="tooltip"
	>
		{#if content}
			{@render content()}
		{:else}
			{text}
		{/if}
	</div>
{/if}
