<script lang="ts">
	import './layout.css';
	import Header from '#lib/components/Header.svelte';
	import Footer from '#lib/components/Footer.svelte';
	import { page } from '$app/state';

	let { children } = $props();

	const SITE = 'https://splitwave.app';
	const OG_IMAGE = `${SITE}/preview.webp`;

	const DEFAULT_SEO = {
		title: 'Splitwave — Audio Routing App for macOS, Linux and Windows',
		description:
			'Free, open-source, node-based audio routing for macOS, Windows and Linux. Route microphones, system audio and apps through effects, plugins and virtual devices.',
		url: SITE,
		robots: 'index, follow',
		image: OG_IMAGE
	};

	const seo = $derived({ ...DEFAULT_SEO, ...(page.data.seo ?? {}) });
	// Error pages (404 etc.) must not claim the home page as canonical or be indexed.
	const isError = $derived(page.status >= 400);
</script>

<svelte:head>
	{#if isError}
		<title>{page.status} — Splitwave</title>
		<meta name="robots" content="noindex" />
	{:else}
		<title>{seo.title}</title>
		<link rel="canonical" href={seo.url} />

		<!-- Primary -->
		<meta name="description" content={seo.description} />
		<meta name="author" content="Splitwave" />
		<meta name="robots" content={seo.robots} />

		<!-- Open Graph -->
		<meta property="og:type" content="website" />
		<meta property="og:site_name" content="Splitwave" />
		<meta property="og:title" content={seo.title} />
		<meta property="og:description" content={seo.description} />
		<meta property="og:url" content={seo.url} />
		<meta property="og:image" content={seo.image} />
		<meta property="og:image:width" content="1280" />
		<meta property="og:image:height" content="800" />
		<meta
			property="og:image:alt"
			content="Splitwave pipeline editor — audio routing for macOS, Linux and Windows"
		/>
		<meta property="og:locale" content="en_US" />

		<!-- Twitter / X -->
		<meta name="twitter:card" content="summary_large_image" />
		<meta name="twitter:title" content={seo.title} />
		<meta name="twitter:description" content={seo.description} />
		<meta name="twitter:image" content={seo.image} />
		<meta
			name="twitter:image:alt"
			content="Splitwave pipeline editor — audio routing for macOS, Linux and Windows"
		/>
	{/if}
</svelte:head>

<a
	href="#content"
	class="sr-only focus:not-sr-only focus:fixed focus:top-2 focus:left-2 focus:z-100 focus:rounded-lg focus:bg-background focus:px-3 focus:py-2 focus:text-sm"
	>Skip to content</a
>

<main class="flex min-h-screen flex-col">
	<Header />
	<div id="content" class="grow">
		{@render children()}
	</div>
	<Footer />
</main>
