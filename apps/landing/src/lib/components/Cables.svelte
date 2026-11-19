<script lang="ts">
	interface Props {
		class?: string;
	}
	let { class: klass = '' }: Props = $props();

	// Colours are the app's channel palette; ch1/ch2 keep the DAW-familiar red/blue.
	const CABLES = [
		{ d: 'M-60 90 C 260 90, 300 210, 620 210 S 1000 120, 1320 160', color: '#ef4444' },
		{ d: 'M-60 150 C 240 150, 300 300, 640 300 S 1020 250, 1320 220', color: '#3b82f6' },
		{ d: 'M-60 320 C 220 320, 280 170, 600 170 S 980 380, 1320 330', color: '#a855f7' },
		{ d: 'M-60 420 C 280 420, 340 500, 660 500 S 1040 430, 1320 470', color: '#f59e0b' },
		{ d: 'M-60 520 C 300 520, 360 380, 700 380 S 1060 540, 1320 500', color: '#22c55e' }
	];

	// A tall viewport slices the wide field down to whatever crosses its middle,
	// so narrow screens get a field authored at their own aspect instead.
	const CABLES_TALL = [
		{ d: 'M-40 90 C 120 140, 300 40, 460 110', color: '#ef4444' },
		{ d: 'M-40 230 C 140 190, 280 300, 460 250', color: '#3b82f6' },
		{ d: 'M-40 400 C 120 450, 300 350, 460 420', color: '#a855f7' },
		{ d: 'M-40 560 C 140 520, 280 630, 460 580', color: '#f59e0b' },
		{ d: 'M-40 710 C 120 760, 300 660, 460 730', color: '#22c55e' }
	];
</script>

<svg
	class={['pointer-events-none absolute inset-0 size-full sm:hidden', klass]}
	viewBox="0 0 420 800"
	preserveAspectRatio="xMidYMid slice"
	aria-hidden="true"
>
	{#each CABLES_TALL as cable (cable.d)}
		<path class="cable" d={cable.d} fill="none" stroke={cable.color} stroke-width="2" />
	{/each}
</svg>

<svg
	class={['pointer-events-none absolute inset-0 hidden size-full sm:block', klass]}
	viewBox="0 0 1200 600"
	preserveAspectRatio="xMidYMid slice"
	aria-hidden="true"
>
	{#each CABLES as cable (cable.d)}
		<path class="cable" d={cable.d} fill="none" stroke={cable.color} stroke-width="2" />
	{/each}
</svg>

<style>
	/* Matches xyflow's animated edge: dasharray 5, dashdraw 0.5s linear. */
	.cable {
		stroke-dasharray: 5;
		animation: dashdraw 0.5s linear infinite;
		opacity: 0.45;
	}

	@keyframes dashdraw {
		from {
			stroke-dashoffset: 10;
		}
	}

	@media (prefers-reduced-motion: reduce) {
		.cable {
			animation: none;
		}
	}
</style>
