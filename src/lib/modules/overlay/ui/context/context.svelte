<script lang="ts">
	import { onDestroy, onMount, type Snippet } from 'svelte';
	import { fly } from 'svelte/transition';
	import { createFloatingActions } from 'svelte-floating-ui';
	import { flip, offset, shift } from 'svelte-floating-ui/dom';
	import { portal } from 'svelte-portal';

	interface Props {
		trigger: Snippet;
		menu: Snippet<[{ close: () => void }]>;
	}

	let { trigger, menu }: Props = $props();

	let open = $state(false);

	const [floatingRef, floatingContent] = createFloatingActions({
		strategy: 'fixed',
		placement: 'bottom-start',
		middleware: [offset({ mainAxis: 4 }), flip(), shift({ padding: 4 })]
	});

	function virtualAt(x: number, y: number) {
		return {
			getBoundingClientRect: () =>
				({
					x, y, left: x, top: y, right: x, bottom: y, width: 0, height: 0,
					toJSON() { return {}; }
				}) as DOMRect
		};
	}

	function onContextMenu(e: MouseEvent) {
		e.preventDefault();
		floatingRef(virtualAt(e.clientX, e.clientY));
		open = true;
	}

	function close() {
		open = false;
	}

	function isInside(t: EventTarget | null): boolean {
		return !!(t instanceof HTMLElement && t.closest('[data-overlay-context]'));
	}

	function onMousedown(e: MouseEvent) {
		if (open && !isInside(e.target)) close();
	}
	function onKeydown(e: KeyboardEvent) {
		if (open && e.key === 'Escape') close();
	}
	function onContext(e: Event) {
		if (open && !isInside(e.target)) close();
	}
	function onScroll() {
		close();
	}
	function onResize() {
		close();
	}

	onMount(() => {
		window.addEventListener('mousedown', onMousedown, { capture: true });
		window.addEventListener('contextmenu', onContext, { capture: true });
		window.addEventListener('keydown', onKeydown);
		window.addEventListener('scroll', onScroll, true);
		window.addEventListener('resize', onResize);
	});
	onDestroy(() => {
		window.removeEventListener('mousedown', onMousedown, { capture: true });
		window.removeEventListener('contextmenu', onContext, { capture: true });
		window.removeEventListener('keydown', onKeydown);
		window.removeEventListener('scroll', onScroll, true);
		window.removeEventListener('resize', onResize);
	});
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div oncontextmenu={onContextMenu} data-overlay-context class="contents">
	{@render trigger()}
</div>

{#if open}
	<div
		use:portal={'#overlays'}
		use:floatingContent
		data-overlay-context
		transition:fly={{ duration: 150, y: 4 }}
		class="z-[200]"
	>
		{@render menu({ close })}
	</div>
{/if}
