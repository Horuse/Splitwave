<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { ClassValue } from 'svelte/elements';
	import ScrollBar from './scroll-bar.svelte';

	type Props = {
		/** Root class: size, background, radius. */
		class?: ClassValue;
		/** Class of the scrolling element: padding, gap, flex. */
		viewportClass?: ClassValue;
		/** The scrolling element, for scrollTo and scroll events. */
		viewport?: HTMLElement | null;
		visibility?: 'hover' | 'always';
		size?: 'sm' | 'md' | 'lg' | 'xl';
		children?: Snippet;
	};

	let { class: className, viewportClass, viewport = $bindable(null), visibility = 'hover', size = 'md', children }: Props = $props();
</script>

<!-- The native scrollbar is hidden and the thumb floats over the content, so it takes no layout width. -->
<div class={['scroll-area', className]} data-visibility={visibility}>
	<div bind:this={viewport} class={['scroll-area-viewport', viewportClass]}>
		{@render children?.()}
	</div>
	<ScrollBar target={viewport} {visibility} {size} />
</div>
