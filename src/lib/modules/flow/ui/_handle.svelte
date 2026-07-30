<script lang="ts">
	import { getContext, type Snippet } from 'svelte';
	import { Handle, type Position } from '@xyflow/svelte';
	import type { ClassValue } from 'svelte/elements';
	import { PREVIEW_CTX } from '$lib/modules/flow/utils';

	interface Props {
		type: 'source' | 'target';
		position: Position;
		id?: string;
		class?: ClassValue;
		style?: string;
		children?: Snippet;
	}
	let { type, position, id, class: klass, style, children }: Props = $props();

	const isPreview = getContext(PREVIEW_CTX) === true;

	// xyflow positions handles through the node context; standalone we place them
	// against the wrapper the call site already made relative.
	const PLACEMENT: Record<Position, string> = {
		left: '-left-1 top-1/2 -translate-y-1/2',
		right: '-right-1 top-1/2 -translate-y-1/2',
		top: '-top-1 left-1/2 -translate-x-1/2',
		bottom: '-bottom-1 left-1/2 -translate-x-1/2'
	};
</script>

{#if isPreview}
	<!-- Handle throws outside a Custom Node component, which the preview grid
	     cannot provide: it renders node components without a flow canvas. -->
	<div class={['absolute', PLACEMENT[position], klass]} {style}>
		{@render children?.()}
	</div>
{:else}
	<Handle {type} {id} {position} class={klass} {style}>
		{@render children?.()}
	</Handle>
{/if}
