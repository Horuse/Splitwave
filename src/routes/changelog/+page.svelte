<script lang="ts">
	import { fly } from 'svelte/transition';
	import { onMount } from 'svelte';
	import { Button, Callout, Card, Separator } from '@splitwave/ui';
	import { openUrl } from '@tauri-apps/plugin-opener';
	import Header from '$lib/components/layout/header.svelte';
	import Markdown from '$lib/components/markdown.svelte';
	import { loadAppInfo } from '$lib/modules/app_info';
	import { changelogSeen, fetchChangelog, type ChangelogEntry } from '$lib/modules/changelog';
	import WhatsNewTabs from '$lib/components/whats_new/whats_new_tabs.svelte';

	let entries = $state<ChangelogEntry[] | null>(null);
	let error = $state<string | null>(null);
	let installed = $state<string | null>(null);

	async function load() {
		error = null;
		entries = null;
		try {
			entries = await fetchChangelog();
			// Opening the changelog is what reading the newest release means.
			if (entries[0]) {
				changelogSeen.setLatest(entries[0].version);
				changelogSeen.markSeen();
			}
		} catch (e) {
			error = e instanceof Error ? e.message : String(e);
		}
	}

	onMount(() => {
		load();
		loadAppInfo().then((info) => (installed = info.appVersion));
	});

	const date = (iso: string) => (iso ? new Date(iso).toLocaleDateString(undefined, { year: 'numeric', month: 'long', day: 'numeric' }) : '');
	let latest = $derived(entries?.[0]);
	let rest = $derived(entries?.slice(1) ?? []);
</script>

<Header />

<div class="flex min-h-0 flex-1 flex-col overflow-y-auto">
	<div class="mx-auto flex w-full max-w-3xl flex-col gap-6 p-8">
		<div class="flex flex-col gap-3">
			<h1 class="text-2xl font-semibold">What's new</h1>
			<WhatsNewTabs />
		</div>

		{#if error}
			<Callout tone="danger">
				Could not load releases from GitHub: {error}
				<Button class="self-start" onclick={load}>Retry</Button>
			</Callout>
		{:else if !entries}
			<p class="text-sm text-neutral-900">Loading releases...</p>
		{:else if latest}
			<Card class="flex flex-col gap-3 p-6">
				<div class="flex flex-wrap items-center gap-2 text-xs">
					<span class="rounded-full bg-brand-soft px-2 py-0.5 font-medium text-brand">Latest</span>
					{#if latest.prerelease}
						<span class="rounded-full border border-dashed border-neutral-500 px-2 py-0.5 font-mono text-[10px] text-neutral-900">pre-release</span>
					{/if}
					{#if latest.version === installed}
						<span class="rounded-full bg-(--lv-2) px-2 py-0.5 font-mono text-[10px] text-neutral-1000">installed</span>
					{/if}
					<span class="ml-auto font-mono text-neutral-900">{date(latest.publishedAt)}</span>
				</div>
				<h2 class="text-2xl font-semibold text-neutral-1200">{latest.name}</h2>
				{#if latest.notes}
					<Separator />
					<Markdown source={latest.notes} class="text-sm text-neutral-1100" />
				{/if}
				<Button class="mt-1 self-start" onclick={() => openUrl(latest.url).catch(() => {})}>View on GitHub</Button>
			</Card>

			{#each rest as entry, i (entry.version)}
				<div in:fly={{ y: 8, duration: 200, delay: Math.min(i, 8) * 40 }}>
					<Card class="flex flex-col gap-3 p-6">
						<div class="flex items-baseline justify-between gap-4">
							<div class="flex items-center gap-2">
								<h3 class="text-lg font-medium">{entry.name}</h3>
								{#if entry.prerelease}
									<span class="rounded-full border border-dashed border-neutral-500 px-2 py-0.5 font-mono text-[10px] text-neutral-900"
										>pre-release</span>
								{/if}
								{#if entry.version === installed}
									<span class="rounded-full bg-brand-soft px-2 py-0.5 font-mono text-[10px] text-brand">installed</span>
								{/if}
							</div>
							<span class="shrink-0 font-mono text-xs text-neutral-900">{date(entry.publishedAt)}</span>
						</div>
						{#if entry.notes}
							<Separator />
							<Markdown source={entry.notes} class="text-sm text-neutral-1100" />
						{/if}
					</Card>
				</div>
			{/each}
		{:else}
			<p class="text-sm text-neutral-900">No releases published yet.</p>
		{/if}
	</div>
</div>
