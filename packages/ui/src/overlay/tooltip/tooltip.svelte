<script lang="ts">
	import type { Snippet } from 'svelte';
	import { writable } from 'svelte/store';
	import { fade } from 'svelte/transition';
	import { arrow as arrowMiddleware, createFloatingActions } from 'svelte-floating-ui';
	import { flip, offset, shift } from 'svelte-floating-ui/dom';
	import { portal } from 'svelte-portal';
	import { OVERLAY_HOST, type Placement } from '../types';
	import { smoothRadius } from '../../attachments';
	import TooltipArrow from './tooltip-arrow.svelte';

	interface Props {
		text?: string;
		content?: Snippet;
		placement?: Placement;
		offsetPx?: number;
		delay?: number;
		arrow?: boolean;
		children: Snippet;
	}

	let { text, content, placement = 'top', offsetPx = 8, delay = 200, arrow = true, children }: Props = $props();

	let open = $state(false);
	let openTimer: ReturnType<typeof setTimeout> | undefined;

	let actual = $state<Placement>(placement);
	let arrowPos = $state<{ x?: number; y?: number }>({});
	let arrowEl = $state<SVGSVGElement>();
	const arrowStore = writable<SVGSVGElement | undefined>(undefined);
	$effect(() => arrowStore.set(arrowEl));

	const [floatingRef, floatingContent] = createFloatingActions({
		strategy: 'fixed',
		placement,
		middleware: [offset(offsetPx), flip(), shift({ padding: 6 }), ...(arrow ? [arrowMiddleware({ element: arrowStore, padding: 8 })] : [])],
		onComputed: ({ placement: p, middlewareData }) => {
			actual = p as Placement;
			arrowPos = { x: middlewareData.arrow?.x, y: middlewareData.arrow?.y };
		}
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

<div use:floatingRef onmouseenter={show} onmouseleave={hide} onfocusin={show} onfocusout={hide} class="inline-flex">
	{@render children()}
</div>

{#if open}
	<div use:portal={OVERLAY_HOST} use:floatingContent transition:fade={{ duration: 100 }} class="pointer-events-none z-[1000]" role="tooltip">
		<div
			data-layer="popover"
			class="relative border border-neutral-400 bg-popover px-2.5 py-1.5 text-xs text-neutral-1100 shadow-lg"
			style="border-radius: var(--smooth-r, 8px)"
			{@attach smoothRadius({ variable: '--smooth-r' })}>
			{#if content}
				{@render content()}
			{:else}
				{text}
			{/if}
		</div>
		{#if arrow}
			<TooltipArrow placement={actual} x={arrowPos.x} y={arrowPos.y} bind:ref={arrowEl} />
		{/if}
	</div>
{/if}
