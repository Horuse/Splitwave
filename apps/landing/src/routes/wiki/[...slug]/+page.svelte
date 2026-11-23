<script lang="ts">
	import { CopyPage, editUrl, getNode, neighbours, PROSE } from '@splitwave/wiki';
	import * as Icons from '#lib/components/icons/index.js';
	import { jsonLdTag } from '#lib/json_ld.js';
	import { SITE, wikiUrl } from '#lib/site.js';

	let { data } = $props();

	let node = $derived(getNode(data.slug)!);
	let article = $derived(node.article!);
	let around = $derived(neighbours(data.slug));
	let Body = $derived(article.component);

	// Parent folders, for the breadcrumb trail and its structured data.
	let crumbs = $derived(
		data.slug
			.split('/')
			.slice(0, -1)
			.map((_, i, parts) => parts.slice(0, i + 1).join('/'))
			.map((slug) => ({
				slug,
				title: getNode(slug)?.title ?? slug,
				linked: !!getNode(slug)?.article
			}))
	);

	let structured = $derived(
		jsonLdTag([
			{
				'@context': 'https://schema.org',
				'@type': 'TechArticle',
				headline: article.title,
				description: data.seo.description,
				url: wikiUrl(data.slug),
				inLanguage: 'en',
				about: {
					'@type': 'SoftwareApplication',
					name: 'Splitwave',
					applicationCategory: 'MultimediaApplication'
				},
				publisher: {
					'@type': 'Organization',
					name: 'Splitwave',
					url: SITE,
					logo: `${SITE}/icon.png`
				}
			},
			{
				'@context': 'https://schema.org',
				'@type': 'BreadcrumbList',
				itemListElement: [
					{ '@type': 'ListItem', position: 1, name: 'Wiki', item: wikiUrl('') },
					...crumbs.map((c, i) => ({
						'@type': 'ListItem',
						position: i + 2,
						name: c.title,
						item: wikiUrl(c.slug)
					})),
					{
						'@type': 'ListItem',
						position: crumbs.length + 2,
						name: article.title,
						item: wikiUrl(data.slug)
					}
				]
			}
		])
	);
</script>

<svelte:head>
	<!-- eslint-disable-next-line svelte/no-at-html-tags -- escaped by jsonLdTag -->
	{@html structured}
</svelte:head>

<article class="flex flex-col gap-6">
	<nav aria-label="Breadcrumb" class="flex flex-wrap items-center gap-1.5 text-xs text-neutral-900">
		<a href="/wiki" class="hover:text-theme">Wiki</a>
		{#each crumbs as c (c.slug)}
			<span aria-hidden="true">/</span>
			{#if c.linked}<a href="/wiki/{c.slug}" class="hover:text-theme">{c.title}</a>{:else}<span>
					{c.title}
				</span>{/if}
		{/each}
	</nav>

	<header class="flex flex-col gap-3">
		<div class="flex flex-wrap items-center justify-between gap-3">
			<h1 class="text-3xl font-bold tracking-tight">{article.title}</h1>
			<CopyPage markdown={article.markdown} url={wikiUrl(data.slug)} />
		</div>
		{#if article.description}
			<p class="text-base leading-relaxed text-neutral-1000">{article.description}</p>
		{/if}
	</header>

	<div class={['text-[15px] leading-relaxed text-neutral-1100', PROSE]}><Body /></div>

	<footer class="mt-6 flex flex-col gap-4 border-t border-neutral-200 pt-6">
		<div class="grid gap-3 sm:grid-cols-2">
			{#if around.prev?.article}
				<a
					href="/wiki/{around.prev.slug}"
					class="card flex flex-col gap-1 hover:border-neutral-500"
				>
					<span class="text-xs text-neutral-900">Previous</span>
					<span class="text-sm font-semibold text-theme">{around.prev.title}</span>
				</a>
			{:else}
				<span></span>
			{/if}
			{#if around.next?.article}
				<a
					href="/wiki/{around.next.slug}"
					class="card flex flex-col gap-1 text-right hover:border-neutral-500"
				>
					<span class="text-xs text-neutral-900">Next</span>
					<span class="text-sm font-semibold text-theme">{around.next.title}</span>
				</a>
			{/if}
		</div>
		<a
			href={editUrl(article)}
			target="_blank"
			rel="noopener noreferrer"
			class="flex w-fit items-center gap-1 text-xs text-neutral-900 hover:text-theme"
		>
			Edit this page on GitHub <Icons.ArrowUpRight class="size-3" />
		</a>
	</footer>
</article>
