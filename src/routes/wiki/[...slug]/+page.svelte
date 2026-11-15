<script lang="ts">
	import { Card } from '@splitwave/ui';
	import { getNode } from '@splitwave/wiki';
	import Header from '$lib/components/layout/header.svelte';

	let { data } = $props();
	let node = $derived(getNode(data.slug)!);
	let crumbs = $derived(
		data.slug
			.split('/')
			.slice(0, -1)
			.map((_, i, parts) => parts.slice(0, i + 1).join('/'))
			.map((slug) => ({ slug, title: getNode(slug)?.title ?? slug }))
	);
</script>

<Header />

<div class="min-h-0 flex-1 overflow-y-auto">
	<article
		class="mx-auto flex w-full max-w-3xl flex-col p-8 text-sm leading-relaxed text-neutral-1100 [&_h2]:mt-6 [&_h2]:text-lg [&_h2]:font-semibold [&_h2]:text-theme [&_p]:mt-3 [&_strong]:font-semibold [&_strong]:text-theme [&_ul]:mt-3 [&_ul]:list-disc [&_ul]:pl-5">
		<nav class="flex flex-wrap items-center gap-1 text-xs text-neutral-900">
			<a href="/wiki" class="hover:text-theme">Wiki</a>
			{#each crumbs as c (c.slug)}
				<span>/</span>
				<a href="/wiki/{c.slug}" class="hover:text-theme">{c.title}</a>
			{/each}
		</nav>
		<h1 class="mt-1 text-2xl font-semibold text-theme">{node.title}</h1>
		{#if node.article}
			<node.article.component />
		{/if}
		{#if node.children.length}
			<div class="mt-6 grid grid-cols-1 gap-3 sm:grid-cols-2">
				{#each node.children as child (child.slug)}
					<a href="/wiki/{child.slug}" class="block">
						<Card class="flex h-full flex-col gap-1 p-5 hover:border-neutral-500">
							<span class="text-sm font-semibold text-theme">{child.title}</span>
							{#if child.article?.description}<span class="text-xs text-neutral-900">{child.article.description}</span>{/if}
						</Card>
					</a>
				{/each}
			</div>
		{/if}
	</article>
</div>
