<script lang="ts">
	import { editUrl, getNode, neighbours } from '@splitwave/wiki';
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
			.map((slug) => ({ slug, title: getNode(slug)?.title ?? slug, linked: !!getNode(slug)?.article }))
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
				about: { '@type': 'SoftwareApplication', name: 'Splitwave', applicationCategory: 'MultimediaApplication' },
				publisher: { '@type': 'Organization', name: 'Splitwave', url: SITE, logo: `${SITE}/icon.png` }
			},
			{
				'@context': 'https://schema.org',
				'@type': 'BreadcrumbList',
				itemListElement: [
					{ '@type': 'ListItem', position: 1, name: 'Wiki', item: wikiUrl('') },
					...crumbs.map((c, i) => ({ '@type': 'ListItem', position: i + 2, name: c.title, item: wikiUrl(c.slug) })),
					{ '@type': 'ListItem', position: crumbs.length + 2, name: article.title, item: wikiUrl(data.slug) }
				]
			}
		])
	);

	// Article typography, applied to the mdsvex output below.
	const PROSE = [
		'text-[15px] leading-relaxed text-neutral-1100',
		'[&_h2]:mt-10 [&_h2]:scroll-mt-24 [&_h2]:text-xl [&_h2]:font-semibold [&_h2]:text-theme',
		'[&_h3]:mt-7 [&_h3]:scroll-mt-24 [&_h3]:text-base [&_h3]:font-semibold [&_h3]:text-theme',
		'[&_p]:mt-3 [&_ul]:mt-3 [&_ul]:list-disc [&_ul]:pl-5 [&_ol]:mt-3 [&_ol]:list-decimal [&_ol]:pl-5 [&_li]:mt-1',
		'[&_a]:text-blue-500 [&_a]:underline [&_a]:underline-offset-2',
		'[&_strong]:font-semibold [&_strong]:text-theme',
		'[&_code]:rounded [&_code]:bg-neutral-200 [&_code]:px-1 [&_code]:py-px [&_code]:font-mono [&_code]:text-[0.88em]',
		'[&_pre]:mt-4 [&_pre]:overflow-x-auto [&_pre]:rounded-xl [&_pre]:border [&_pre]:border-neutral-300 [&_pre]:bg-neutral-100 [&_pre]:p-4 [&_pre_code]:bg-transparent [&_pre_code]:p-0',
		'[&_blockquote]:mt-4 [&_blockquote]:border-l-2 [&_blockquote]:border-neutral-400 [&_blockquote]:pl-4 [&_blockquote]:text-neutral-1000',
		'[&_table]:mt-4 [&_table]:w-full [&_table]:text-sm [&_th]:border-b [&_th]:border-neutral-300 [&_th]:py-2 [&_th]:pr-4 [&_th]:text-left [&_th]:font-semibold [&_td]:border-b [&_td]:border-neutral-200 [&_td]:py-2 [&_td]:pr-4',
		'[&_kbd]:rounded [&_kbd]:border [&_kbd]:border-neutral-400 [&_kbd]:bg-neutral-100 [&_kbd]:px-1.5 [&_kbd]:font-sans [&_kbd]:text-xs',
		'[&_img]:mt-4 [&_img]:rounded-xl [&_img]:border [&_img]:border-neutral-300'
	];
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
			{#if c.linked}<a href="/wiki/{c.slug}" class="hover:text-theme">{c.title}</a>{:else}<span>{c.title}</span>{/if}
		{/each}
	</nav>

	<header class="flex flex-col gap-3">
		<h1 class="text-3xl font-bold tracking-tight">{article.title}</h1>
		{#if article.description}
			<p class="text-base leading-relaxed text-neutral-1000">{article.description}</p>
		{/if}
	</header>

	<div class={PROSE}><Body /></div>

	<footer class="mt-6 flex flex-col gap-4 border-t border-neutral-200 pt-6">
		<div class="grid gap-3 sm:grid-cols-2">
			{#if around.prev?.article}
				<a href="/wiki/{around.prev.slug}" class="card flex flex-col gap-1 hover:border-neutral-500">
					<span class="text-xs text-neutral-900">Previous</span>
					<span class="text-sm font-semibold text-theme">{around.prev.title}</span>
				</a>
			{:else}
				<span></span>
			{/if}
			{#if around.next?.article}
				<a href="/wiki/{around.next.slug}" class="card flex flex-col gap-1 text-right hover:border-neutral-500">
					<span class="text-xs text-neutral-900">Next</span>
					<span class="text-sm font-semibold text-theme">{around.next.title}</span>
				</a>
			{/if}
		</div>
		<a href={editUrl(article)} target="_blank" rel="noopener noreferrer" class="flex w-fit items-center gap-1 text-xs text-neutral-900 hover:text-theme">
			Edit this page on GitHub <Icons.ArrowUpRight class="size-3" />
		</a>
	</footer>
</article>
