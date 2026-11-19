<script lang="ts">
	import { onDestroy } from 'svelte';
	import { Card } from '@splitwave/ui';
	import Header from '$lib/components/layout/header.svelte';
	import WhatsNewTabs from '$lib/components/whats_new/whats_new_tabs.svelte';
	import { announcementStore } from '$lib/modules/announcements';
	import SeverityBadge from './severity_badge.svelte';

	let items = $derived(announcementStore.all);

	// "New" stays on the cards while the list is open; leaving it counts them as read.
	let unreadOnEntry = $state<Set<string> | null>(null);
	$effect(() => {
		if (unreadOnEntry === null && items.length > 0) {
			unreadOnEntry = new Set(items.filter((a) => announcementStore.isUnread(a.id)).map((a) => a.id));
		}
	});
	onDestroy(() => announcementStore.markSeen(items.map((a) => a.id)));

	const date = (iso?: string) => (iso ? new Date(iso).toLocaleDateString(undefined, { year: 'numeric', month: 'long', day: 'numeric' }) : '');
</script>

<Header />

<div class="flex min-h-0 flex-1 flex-col overflow-y-auto">
	<div class="mx-auto flex w-full max-w-3xl flex-col gap-6 p-8">
		<div class="flex flex-col gap-3">
			<h1 class="text-2xl font-semibold">What's new</h1>
			<WhatsNewTabs />
		</div>

		{#if announcementStore.isLoading && items.length === 0}
			<p class="text-sm text-neutral-900">Loading announcements...</p>
		{:else if items.length === 0}
			<p class="text-sm text-neutral-900">No announcements right now.</p>
		{:else}
			<div class="flex flex-col gap-3">
				{#each items as item (item.id)}
					<a href="/announcements/{encodeURIComponent(item.id)}" class="group">
						<Card class="flex flex-col gap-2 p-5 group-hover:border-neutral-500">
							<div class="flex items-center gap-2">
								<SeverityBadge announcement={item} />
								{#if unreadOnEntry?.has(item.id)}
									<span class="flex items-center gap-1 text-[10px] font-semibold text-brand">
										<span class="size-1.5 rounded-full bg-brand"></span>New
									</span>
								{/if}
								<span class="ml-auto font-mono text-xs text-neutral-800">{date(item.updatedAt)}</span>
							</div>
							<h2 class="text-lg font-medium text-neutral-1200">{item.title}</h2>
							<p class="text-sm text-neutral-900">{item.message}</p>
							{#if item.markdown}
								<span class="text-xs font-medium text-neutral-1100 group-hover:underline">Read more</span>
							{/if}
						</Card>
					</a>
				{/each}
			</div>
		{/if}
	</div>
</div>
