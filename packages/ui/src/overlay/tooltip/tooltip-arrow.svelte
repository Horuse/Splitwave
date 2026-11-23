<script lang="ts">
	import type { Placement } from '../types';

	interface Props {
		placement: Placement;
		x?: number;
		y?: number;
		ref?: SVGSVGElement;
	}

	let { placement, x, y, ref = $bindable() }: Props = $props();

	const W = 14;
	const H = 7;
	const STROKE = 2;
	const D = `M0,0 H${W} L${W / 2},${H} Z`;
	const clipId = `tooltip-arrow-${Math.random().toString(36).slice(2, 9)}`;

	let side = $derived(placement.split('-')[0] as 'top' | 'bottom' | 'left' | 'right');
	let vertical = $derived(side === 'top' || side === 'bottom');
	let rotate = $derived({ top: '', bottom: 'rotate(180deg)', left: 'rotate(-90deg)', right: 'rotate(90deg)' }[side]);
</script>

<!-- The clip hides the stroke along the edge that meets the bubble, so the two read as one shape. -->
<svg
	bind:this={ref}
	aria-hidden="true"
	width={W + STROKE}
	height={W}
	viewBox="0 0 {W} {W}"
	class="pointer-events-none absolute"
	style:left={x != null ? `${x}px` : undefined}
	style:top={y != null ? `${y}px` : undefined}
	style:transform={rotate}
	style={`${side}: ${vertical ? 'calc(100% - 1px)' : `calc(100% - ${STROKE / 2 + 1}px)`}`}>
	<path clip-path="url(#{clipId})" fill="none" class="stroke-neutral-400" stroke-width={STROKE} d={D} />
	<path class="fill-popover" d={D} />
	<clipPath id={clipId}>
		<rect x={-STROKE / 2} y={STROKE / 2} width={W + STROKE} height={W} />
	</clipPath>
</svg>
