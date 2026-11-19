<script lang="ts">
	import ReleaseNotes from '#lib/components/ReleaseNotes.svelte';
	import { formatDate } from '#lib/releases.js';
	import * as Icons from '#lib/components/icons/index.js';

	let { data } = $props();

	const GITHUB_RELEASES = 'https://github.com/Horuse/Splitwave/releases';
</script>

<div class="mx-auto flex max-w-4xl flex-col gap-12 px-6 py-16">
	<a
		href="/"
		class="inline-block self-start text-sm text-neutral-900 transition-colors hover:text-theme"
		>&larr; Back</a
	>

	<div class="flex flex-col gap-2">
		<h1 class="text-3xl font-bold">Changelog</h1>
		<p class="text-sm text-neutral-900">
			Every Splitwave release, newest first. Pulled straight from GitHub.
		</p>
	</div>

	{#if !data.releases}
		<div class="card flex flex-col gap-2">
			<p class="text-sm font-semibold text-theme">Release notes are unavailable right now.</p>
			<p class="text-sm text-neutral-1000">
				GitHub could not be reached. You can read them
				<a
					href={GITHUB_RELEASES}
					target="_blank"
					rel="noopener noreferrer"
					class="text-blue-500 underline underline-offset-2">on GitHub</a
				> instead.
			</p>
		</div>
	{:else}
		<ol class="flex flex-col">
			{#each data.releases as release, i (release.tag)}
				<li id={release.tag} class="grid scroll-mt-24 gap-x-10 gap-y-4 sm:grid-cols-[10rem_1fr]">
					<div class="flex flex-col gap-2 sm:sticky sm:top-24 sm:self-start">
						<div class="flex items-center flex-wrap gap-2">
							<h2 class="text-2xl font-bold tracking-tight">{release.tag}</h2>
							{#if release.tag === data.latestTag}
								<span
									class="rounded-full border border-neutral-300 bg-neutral-200 px-2 py-0.5 text-xs whitespace-nowrap text-neutral-1000"
									>Latest</span
								>
							{:else if release.prerelease}
								<span
									class="rounded-full border border-dashed border-amber-500/60 px-2 py-0.5 text-xs whitespace-nowrap text-amber-500"
									>Pre-release</span
								>
							{/if}
						</div>
						<time datetime={release.publishedAt} class="text-xs text-neutral-900">
							{formatDate(release.publishedAt)}
						</time>
						<a
							href={release.url}
							target="_blank"
							rel="noopener noreferrer"
							class="mt-1 inline-flex items-center gap-1 self-start text-xs text-neutral-900 transition-colors hover:text-theme"
						>
							GitHub
							<Icons.ArrowUpRight class="size-3 fill-current" />
						</a>
					</div>
					<!-- The dashed rail echoes the cables between nodes on the landing page. -->
					<div class="relative border-dashed border-neutral-400 pb-14 sm:border-l sm:pl-10">
						<span
							class="absolute top-2.5 -left-[5px] hidden size-2.5 rounded-full bg-neutral-600 sm:block {release.tag ===
							data.latestTag
								? 'bg-blue-500!'
								: ''}"
						></span>
						<ReleaseNotes body={release.body} />
					</div>
				</li>
			{/each}
		</ol>

		<p class="text-sm text-neutral-900">
			Older versions and checksums live on
			<a
				href={GITHUB_RELEASES}
				target="_blank"
				rel="noopener noreferrer"
				class="text-blue-500 underline underline-offset-2">GitHub releases</a
			>.
		</p>
	{/if}
</div>
