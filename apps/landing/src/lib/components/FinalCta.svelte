<script lang="ts">
	import * as Icons from '#lib/components/icons/index.js';

	const GITHUB = 'https://github.com/Horuse/Splitwave';

	// Sound waves rising from the bottom edge, in the channel palette.
	const RINGS = [
		{ r: 120, color: '#22c55e' },
		{ r: 200, color: '#f59e0b' },
		{ r: 280, color: '#a855f7' },
		{ r: 360, color: '#3b82f6' },
		{ r: 440, color: '#ef4444' }
	];
</script>

<section class="mx-auto max-w-3xl px-6 pb-24">
	<div
		class="relative isolate flex flex-col items-center gap-5 overflow-hidden rounded-[2rem] border border-neutral-300 bg-neutral-100 px-12 sm:px-24 py-14 text-center shadow-2xl shadow-black/25"
	>
		<svg
			class="pointer-events-none absolute inset-x-0 bottom-0 -z-10 h-full w-full"
			viewBox="0 0 800 360"
			preserveAspectRatio="xMidYMax slice"
			aria-hidden="true"
		>
			{#each RINGS as ring (ring.r)}
				<circle
					cx="400"
					cy="420"
					r={ring.r}
					fill="none"
					stroke={ring.color}
					stroke-width="1.5"
					stroke-dasharray="6 8"
					stroke-opacity="0.35"
				/>
			{/each}
		</svg>
		<div class="glow pointer-events-none absolute inset-0 -z-10" aria-hidden="true"></div>

		<h2 class="text-3xl font-bold tracking-tight">Build your first pipeline</h2>
		<p class="max-w-md text-pretty text-neutral-1000">
			Free and open source, for macOS, Linux and Windows.
		</p>
		<div class="flex flex-wrap items-center justify-center gap-3">
			<a href="/download" class="btn-primary text-base">Download</a>
			<a href={GITHUB} target="_blank" rel="noopener noreferrer" class="btn-secondary text-base">
				View on GitHub
				<Icons.ArrowUpRight class="size-3" />
			</a>
		</div>
	</div>
</section>

<style>
	/* Keeps the rings off the copy, like the mask over the hero cables. */
	.glow {
		background: radial-gradient(
			ellipse 70% 55% at 50% 40%,
			var(--color-neutral-100) 25%,
			transparent 100%
		);
	}
</style>
