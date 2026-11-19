<script lang="ts">
	const GITHUB = 'https://github.com/Horuse/Splitwave';

	import * as Icons from '#lib/components/icons/index.js';
	import Cables from '#lib/components/Cables.svelte';
	import type { Release } from '#lib/releases.js';

	let { release }: { release: Release | null } = $props();
</script>

<div class="relative overflow-hidden">
	<Cables class="opacity-70" />
	<!-- Keeps the cable field from competing with the headline. -->
	<div class="mask pointer-events-none absolute inset-0"></div>

	<section
		class="relative mx-auto flex max-w-4xl flex-col items-center px-6 pt-24 pb-5 text-center"
	>
		<div class="mb-8 flex justify-center">
			<img
				src="/icon.svg"
				alt="Splitwave"
				title="Splitwave"
				class="size-24 rounded-[14px] shadow-xl shadow-black/20"
			/>
		</div>
		{#if release}
			<a
				href="/changelog#{release.tag}"
				class="mb-5 inline-flex items-center gap-2 rounded-full border border-neutral-300 px-3 py-1 text-xs text-neutral-1000 transition-colors hover:border-neutral-500 hover:text-theme"
			>
				<span class="size-1.5 rounded-full bg-blue-500"></span>
				v{release.version} &middot; What's new
			</a>
		{/if}
		<h1 class="mb-4 text-5xl font-bold tracking-tight text-balance">
			Audio routing that works the way you think
		</h1>
		<p class="mb-3 max-w-xl text-xl text-neutral-1000">
			Free, open-source, node-based audio routing that works the way you think for macOS, Linux and
			Windows.
		</p>
		<p class="mb-10 max-w-lg text-pretty text-neutral-900">
			Build a node graph of inputs, effects, and outputs. Process any audio source in real time with
			a flexible processing pipeline.
		</p>
		<div class="flex flex-wrap items-center justify-center gap-3">
			<a href="/download" class="btn-primary text-base"> Download </a>
			<a href={GITHUB} target="_blank" rel="noopener noreferrer" class="btn-secondary text-base">
				View on GitHub
				<Icons.ArrowUpRight class="size-3" />
			</a>
		</div>

		<!-- Overflowing the text column is the intended look, but only once there is
		     room for it; below that the shot just gets cropped to nothing. -->
		<div class="mt-20 w-full pb-10 xl:w-350 xl:px-8">
			<enhanced:img
				src="../../../static/preview.webp?format=avif;webp&w=3248;2800;2200;1600;1000;700"
				alt="Splitwave audio routing pipeline editor on macOS"
				title="Splitwave pipeline editor"
				class="w-full"
				sizes="min(2000px, 100vw)"
				fetchpriority="high"
			/>
		</div>
	</section>
</div>

<style>
	/* The headline block is proportionally taller on a phone, so the clearing
	   is flatter there and the cables above and below it survive. */
	.mask {
		background: radial-gradient(
			ellipse 95% 32% at 50% 34%,
			var(--color-background) 30%,
			transparent 100%
		);
	}

	@media (width >= 40rem) {
		.mask {
			background: radial-gradient(
				ellipse 60% 55% at 50% 42%,
				var(--color-background) 30%,
				transparent 100%
			);
		}
	}
</style>
