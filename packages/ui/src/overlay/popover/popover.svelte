<script lang="ts">
	import type { Snippet } from 'svelte';
	import { fly } from 'svelte/transition';
	import { createFloatingActions } from 'svelte-floating-ui';
	import { flip, offset, shift } from 'svelte-floating-ui/dom';
	import { portal } from 'svelte-portal';
	import { OVERLAY_HOST, type Placement } from '../types';

	interface Props {
		open?: boolean;
		placement?: Placement;
		offsetPx?: number;
		trigger: Snippet;
		children: Snippet;
		onOpenChange?: (open: boolean) => void;
	}

	let { open = $bindable(false), placement = 'bottom-start', offsetPx = 6, trigger, children, onOpenChange }: Props = $props();

	let triggerEl = $state<HTMLElement>();
	let contentEl = $state<HTMLElement>();

	const [floatingRef, floatingContent] = createFloatingActions({
		strategy: 'fixed',
		placement,
		middleware: [offset(offsetPx), flip(), shift({ padding: 6 })]
	});

	function setOpen(v: boolean) {
		open = v;
		onOpenChange?.(v);
	}

	function onTriggerClick() {
		setOpen(!open);
	}

	function onMousedown(e: MouseEvent) {
		if (!open) return;
		const t = e.target as Node;
		if (triggerEl?.contains(t) || contentEl?.contains(t)) return;
		setOpen(false);
	}

	function onKeydown(e: KeyboardEvent) {
		if (open && e.key === 'Escape') setOpen(false);
	}

	// An effect, not onMount/onDestroy: onDestroy also runs during SSR, where window does not exist.
	$effect(() => {
		window.addEventListener('mousedown', onMousedown, { capture: true });
		window.addEventListener('keydown', onKeydown);
		return () => {
			window.removeEventListener('mousedown', onMousedown, { capture: true });
			window.removeEventListener('keydown', onKeydown);
		};
	});
</script>

<div bind:this={triggerEl} use:floatingRef onclick={onTriggerClick} class="inline-flex">
	{@render trigger()}
</div>

{#if open}
	<div bind:this={contentEl} use:portal={OVERLAY_HOST} use:floatingContent transition:fly={{ duration: 200, y: 5 }} class="z-[150]">
		{@render children()}
	</div>
{/if}
