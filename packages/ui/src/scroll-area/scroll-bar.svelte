<script lang="ts">
	type Props = {
		/** The scrolling element; the thumb is drawn over it. */
		target: HTMLElement | null;
		/** hover: thumb shows on hover over the parent .scroll-area or while scrolling; always: always visible. */
		visibility?: 'hover' | 'always';
		/** Thumb width: sm 4px, md 6px (default), lg 8px, xl 10px; +2px while the pointer is over the track. */
		size?: 'sm' | 'md' | 'lg' | 'xl';
	};

	let { target: viewport, visibility = 'hover', size = 'md' }: Props = $props();

	const MIN_THUMB = 32;
	const INSET = 4;
	const HIDE_DELAY = 900;

	let thumbTop = $state(0);
	let thumbHeight = $state(0);
	let scrollable = $state(false);
	let dragging = $state(false);
	let scrolling = $state(false);
	let hideTimer: ReturnType<typeof setTimeout> | undefined;

	function update() {
		if (!viewport) return;
		const { scrollTop, scrollHeight, clientHeight } = viewport;
		const overflow = scrollHeight - clientHeight;
		scrollable = overflow > 1;
		if (!scrollable) return;
		const trackHeight = clientHeight - INSET * 2;
		thumbHeight = Math.max(MIN_THUMB, (clientHeight / scrollHeight) * trackHeight);
		const free = trackHeight - thumbHeight;
		thumbTop = free > 0 ? (scrollTop / overflow) * free : 0;
	}

	function onscroll() {
		update();
		scrolling = true;
		clearTimeout(hideTimer);
		hideTimer = setTimeout(() => (scrolling = false), HIDE_DELAY);
	}

	// Children resize (a section opens, items filter out) without the viewport itself resizing, so watch them too.
	$effect(() => {
		const el = viewport;
		if (!el) return;
		el.addEventListener('scroll', onscroll, { passive: true });
		const resize = new ResizeObserver(update);
		const watch = () => {
			resize.disconnect();
			resize.observe(el);
			for (const child of Array.from(el.children)) resize.observe(child);
			update();
		};
		const mutation = new MutationObserver(watch);
		mutation.observe(el, { childList: true });
		watch();
		return () => {
			el.removeEventListener('scroll', onscroll);
			resize.disconnect();
			mutation.disconnect();
			clearTimeout(hideTimer);
		};
	});

	function startDrag(event: PointerEvent) {
		if (!viewport) return;
		event.preventDefault();
		event.stopPropagation();
		const handle = event.currentTarget as HTMLElement;
		const startY = event.clientY;
		const startScroll = viewport.scrollTop;
		handle.setPointerCapture(event.pointerId);
		dragging = true;

		const move = (e: PointerEvent) => {
			if (!viewport) return;
			const free = viewport.clientHeight - INSET * 2 - thumbHeight;
			const overflow = viewport.scrollHeight - viewport.clientHeight;
			if (free > 0) viewport.scrollTop = startScroll + ((e.clientY - startY) * overflow) / free;
		};
		const stop = () => {
			dragging = false;
			handle.removeEventListener('pointermove', move);
			handle.removeEventListener('pointerup', stop);
			handle.removeEventListener('pointercancel', stop);
		};
		handle.addEventListener('pointermove', move);
		handle.addEventListener('pointerup', stop);
		handle.addEventListener('pointercancel', stop);
	}

	function jump(event: PointerEvent) {
		if (!viewport) return;
		const track = event.currentTarget as HTMLElement;
		const rect = track.getBoundingClientRect();
		const free = rect.height - thumbHeight;
		if (free <= 0) return;
		const ratio = (event.clientY - rect.top - thumbHeight / 2) / free;
		viewport.scrollTop = Math.min(1, Math.max(0, ratio)) * (viewport.scrollHeight - viewport.clientHeight);
	}
</script>

{#if scrollable}
	<!-- svelte-ignore a11y_no_static_element_interactions -->
	<div class="scroll-area-track" data-active={scrolling || dragging || undefined} data-visibility={visibility} data-size={size} onpointerdown={jump}>
		<!-- svelte-ignore a11y_no_static_element_interactions -->
		<div class="scroll-area-thumb" style:height="{thumbHeight}px" style:transform="translateY({thumbTop}px)" onpointerdown={startDrag}></div>
	</div>
{/if}
