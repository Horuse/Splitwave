<script lang="ts">
	import { getArticle } from '@splitwave/wiki';
	import Header from '$lib/components/layout/header.svelte';

	let { data } = $props();
	let article = $derived(getArticle(data.slug)!);
</script>

<Header />

<div class="min-h-0 flex-1 overflow-y-auto">
	<article
		class="mx-auto flex w-full max-w-3xl flex-col p-8 text-sm leading-relaxed text-neutral-1100 [&_a]:underline [&_h2]:mt-6 [&_h2]:text-lg [&_h2]:font-semibold [&_h2]:text-theme [&_p]:mt-3 [&_strong]:font-semibold [&_strong]:text-theme [&_ul]:mt-3 [&_ul]:list-disc [&_ul]:pl-5">
		<a href="/wiki" class="text-xs text-neutral-900 no-underline! hover:text-theme">Wiki</a>
		<h1 class="mt-1 text-2xl font-semibold text-theme">{article.title}</h1>
		<article.component />
	</article>
</div>
