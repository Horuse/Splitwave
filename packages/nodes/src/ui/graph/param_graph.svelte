<script lang="ts" module>
	export interface GraphScale {
		/** Data value to SVG x / y. */
		x: (v: number) => number;
		y: (v: number) => number;
		width: number;
		height: number;
	}
</script>

<script lang="ts">
	import type { Snippet } from 'svelte';

	interface Props {
		x: [number, number];
		y: [number, number];
		/** Values that get a grid line and a label on each axis. */
		xTicks?: number[];
		yTicks?: number[];
		xLabel?: string;
		yLabel?: string;
		/** Tick label text; the raw value when absent. */
		xFormat?: (v: number) => string;
		yFormat?: (v: number) => string;
		/** Drawing area in SVG units; the graph scales to its container's width. */
		width?: number;
		height?: number;
		/** Caption under the plot: the key setting, a state, a live reading. */
		badge?: Snippet;
		children: Snippet<[GraphScale]>;
	}

	let {
		x: xr,
		y: yr,
		xTicks = [],
		yTicks = [],
		xLabel,
		yLabel,
		xFormat = String,
		yFormat = String,
		width = 200,
		height = 96,
		badge,
		children
	}: Props = $props();

	// Room on the left and bottom for tick labels, inside the same viewBox.
	const PAD_L = 18;
	const PAD_B = 10;

	let scale = $derived<GraphScale>({
		x: (v) => PAD_L + ((v - xr[0]) / (xr[1] - xr[0])) * (width - PAD_L),
		y: (v) => ((yr[1] - v) / (yr[1] - yr[0])) * (height - PAD_B),
		width,
		height
	});

	let plotW = $derived(width - PAD_L);
	let plotH = $derived(height - PAD_B);
</script>

<div class="w-full overflow-hidden rounded-md border border-theme/5 bg-neutral-100 select-none">
	<svg viewBox="0 0 {width} {height}" class="block w-full" aria-hidden="true">
		<defs>
			<clipPath id="plot-{xr[0]}-{yr[0]}-{width}-{height}">
				<rect x={PAD_L} y="0" width={plotW} height={plotH} />
			</clipPath>
		</defs>

		{#each xTicks as t (t)}
			<line x1={scale.x(t)} y1="0" x2={scale.x(t)} y2={plotH} class="stroke-neutral-300" stroke-width="0.5" />
			<text x={scale.x(t)} y={height - 2} text-anchor="middle" class="fill-neutral-500 font-mono" font-size="6">{xFormat(t)}</text>
		{/each}
		{#each yTicks as t (t)}
			<line x1={PAD_L} y1={scale.y(t)} x2={width} y2={scale.y(t)} class="stroke-neutral-300" stroke-width="0.5" />
			<text x={PAD_L - 3} y={scale.y(t) + 2} text-anchor="end" class="fill-neutral-500 font-mono" font-size="6">{yFormat(t)}</text>
		{/each}
		<line x1={PAD_L} y1="0" x2={PAD_L} y2={plotH} class="stroke-neutral-400" stroke-width="0.5" />
		<line x1={PAD_L} y1={plotH} x2={width} y2={plotH} class="stroke-neutral-400" stroke-width="0.5" />

		{#if xLabel}
			<text x={width - 2} y={plotH - 3} text-anchor="end" class="fill-neutral-500" font-size="6">{xLabel}</text>
		{/if}
		{#if yLabel}
			<text x={PAD_L + 3} y="7" class="fill-neutral-500" font-size="6">{yLabel}</text>
		{/if}

		<g clip-path="url(#plot-{xr[0]}-{yr[0]}-{width}-{height})">
			{@render children(scale)}
		</g>
	</svg>

	{#if badge}
		<div class="flex items-center justify-end gap-2 border-t border-theme/5 px-1.5 py-0.5 font-mono text-[9px] text-neutral-800 tabular-nums">
			{@render badge()}
		</div>
	{/if}
</div>
