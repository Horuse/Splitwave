<script lang="ts">
	import { page } from '$app/state';
	import { Button } from '@splitwave/ui';
	import ArrowLeft from '~icons/fluent/arrow-left-16-regular';
	import Header from '$lib/components/layout/header.svelte';
	import Markdown from '$lib/components/markdown.svelte';
	import { announcementStore, handleAnnouncementAction } from '$lib/modules/announcements';
	import SeverityBadge from '../severity_badge.svelte';

	let id = $derived(decodeURIComponent(page.params.id ?? ''));
	let item = $derived(announcementStore.all.find((a) => a.id === id) ?? null);

	// Reading the article is what clears its "new" dot.
	$effect(() => {
		if (item) announcementStore.markSeen([item.id]);
	});

	const date = (iso?: string) => (iso ? new Date(iso).toLocaleDateString(undefined, { year: 'numeric', month: 'long', day: 'numeric' }) : '');
</script>

<Header />

<div class="flex min-h-0 flex-1 flex-col overflow-y-auto">
	<article class="mx-auto flex w-full max-w-2xl flex-col gap-5 p-8">
		<a href="/announcements" class="flex w-fit items-center gap-1 text-xs text-neutral-800 hover:text-neutral-1100">
			<ArrowLeft class="size-3.5" />All announcements
		</a>

		{#if item}
			<header class="flex flex-col gap-3">
				<div class="flex items-center gap-2">
					<SeverityBadge announcement={item} />
					<span class="font-mono text-xs text-neutral-800">{date(item.updatedAt)}</span>
				</div>
				<h1 class="text-3xl font-semibold text-neutral-1200">{item.title}</h1>
				<p class="text-base text-neutral-900">{item.message}</p>
			</header>

			{#if item.markdown}
				<Markdown source={item.markdown} variant="article" class="text-sm text-neutral-1100" />
			{/if}

			{#if item.action}
				<Button variant="solid" class="self-start" onclick={() => item && handleAnnouncementAction(item).catch(() => {})}>{item.action.label}</Button>
			{/if}
		{:else if announcementStore.isLoading}
			<p class="text-sm text-neutral-900">Loading...</p>
		{:else}
			<p class="text-sm text-neutral-900">This announcement is no longer available.</p>
		{/if}
	</article>
</div>
