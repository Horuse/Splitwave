<script lang="ts">
	import { tick, type Snippet } from 'svelte';

	// activeKey changes whenever the selected child changes, so the marker can re-measure.
	let { activeKey, children }: { activeKey?: string | null; children?: Snippet } = $props();

	let el = $state<HTMLDivElement>();
	let top = $state(0);
	let height = $state(0);
	let visible = $state(false);

	$effect(() => {
		void activeKey;
		tick().then(() => {
			const item = el?.querySelector<HTMLElement>(':scope > [data-active="true"]');
			visible = !!item;
			if (!item) return;
			top = item.offsetTop;
			height = item.offsetHeight;
		});
	});
</script>

<div bind:this={el} class="sidebar-group">
	<span class="sidebar-group-marker" class:opacity-0={!visible} style:height="{height}px" style:transform="translateY({top}px)"></span>
	{@render children?.()}
</div>
