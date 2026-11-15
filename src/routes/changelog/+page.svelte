<script lang="ts">
	import { fly } from 'svelte/transition';
	import { onMount } from 'svelte';
	import { Banner, Button, Callout, Card, Separator } from '@splitwave/ui';
	import { openUrl } from '@tauri-apps/plugin-opener';
	import Header from '$lib/components/layout/header.svelte';
	import Markdown from '$lib/components/markdown.svelte';
	import { loadAppInfo } from '$lib/modules/app_info';
	import { fetchChangelog, type ChangelogEntry } from '$lib/modules/changelog';

	let entries = $state<ChangelogEntry[] | null>(null);
	let error = $state<string | null>(null);
	let installed = $state<string | null>(null);

	async function load() {
		error = null;
		entries = null;
		try {
			entries = await fetchChangelog();
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
		<h1 class="text-2xl font-semibold">Changelog</h1>

		{#if error}
			<Callout tone="danger">
				Could not load releases from GitHub: {error}
				<Button class="self-start" onclick={load}>Retry</Button>
			</Callout>
		{:else if !entries}
			<p class="text-sm text-neutral-900">Loading releases...</p>
		{:else if latest}
			<Banner class="flex flex-col gap-3 px-8 pt-10 pb-16">
				<div class="flex items-center gap-2 font-mono text-xs text-white/80">
					<span>Latest release</span>
					<span>{date(latest.publishedAt)}</span>
					{#if latest.prerelease}<span class="rounded-full bg-white/20 px-2 py-0.5">pre-release</span>{/if}
					{#if latest.version === installed}<span class="rounded-full bg-white/20 px-2 py-0.5">installed</span>{/if}
				</div>
				<h2 class="text-3xl font-medium">{latest.name}</h2>
				{#if latest.notes}
					<Markdown source={latest.notes} class="max-w-2xl text-sm text-white/90" />
				{/if}
				<Button class="mt-2 self-start border-transparent bg-white text-black hover:bg-white/90" onclick={() => openUrl(latest.url).catch(() => {})}>
					View on GitHub
				</Button>
			</Banner>

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
