<script lang="ts">
	import Checksum from '#lib/components/Checksum.svelte';
	import Section from '#lib/components/Section.svelte';
	import * as Icons from '#lib/components/icons/index.js';

	import { formatSize, pickAsset, type Release } from '#lib/releases.js';

	let { release }: { release: Release | null } = $props();

	// Matches the platform accent used by the selector on /download.
	const ACTIVE = 'border-blue-500/50 mix-bg-blue-500 !text-blue-400';

	const RELEASE = $derived(release?.url ?? 'https://github.com/Horuse/Splitwave/releases');
	const asset = $derived(release && pickAsset(release.assets, '-setup.exe'));
</script>

<div class="flex flex-col gap-12">
	<Section label="Download">
		{#if !asset}
			<div
				class="flex flex-col gap-2 rounded-xl border border-dashed border-amber-500/50 px-4 py-3"
			>
				<p class="inline-flex items-center gap-2 font-semibold text-theme">
					<Icons.Warning class="size-4 text-amber-500" />
					Downloads are unavailable right now.
				</p>
				<p class="text-sm text-neutral-1000">
					GitHub could not be reached. Grab the latest installer from
					<a
						href={RELEASE}
						target="_blank"
						rel="noopener noreferrer"
						class="text-theme underline decoration-neutral-500 underline-offset-2 transition-colors hover:decoration-theme"
					>
						the releases page
					</a>.
				</p>
			</div>
		{:else}
			<div class="flex flex-wrap gap-2">
				<span class={['btn-tab', ACTIVE]}>
					<Icons.Desktop class="size-4" />
					x64
				</span>
			</div>

			<div class="flex flex-col gap-4">
				<p class="text-sm text-neutral-1000">Windows 10 and 11 (64-bit)</p>
				<div class="flex flex-wrap items-center gap-3">
					<a href={asset.url} class="btn-primary break-all">
						<Icons.ArrowDownload class="size-4 shrink-0" />
						Download {asset.name}
					</a>
					<span class="inline-flex items-center gap-1.5 text-xs text-neutral-900">
						<Icons.HardDrive class="size-3.5" />
						{formatSize(asset.size)}
					</span>
					{#if asset.sha256}
						<Checksum sha256={asset.sha256} />
					{/if}
				</div>
				<p class="text-xs text-neutral-900">
					64-bit x86_64 installer. Run the setup and follow the prompts.
				</p>
			</div>
		{/if}

		<p class="text-sm text-neutral-900">
			Looking for older versions? See all
			<a
				href={RELEASE}
				target="_blank"
				rel="noopener noreferrer"
				class="text-theme underline decoration-neutral-500 underline-offset-2 transition-colors hover:decoration-theme"
			>
				releases on GitHub
				<Icons.ArrowUpRight class="inline size-3 fill-current" />
			</a>.
		</p>
	</Section>

	<Section label="Install">
		<ol class="flex flex-col gap-5 text-sm leading-relaxed text-neutral-1000">
			<li class="flex gap-3">
				<span class="mt-0.5 w-4 shrink-0 font-mono text-xs text-neutral-900"> 1 </span>
				<span>
					<span class="font-semibold text-theme">Run</span> the downloaded
					<span class="font-medium text-theme">installer</span>.
				</span>
			</li>
			<li class="flex gap-3">
				<span class="mt-0.5 w-4 shrink-0 font-mono text-xs text-neutral-900"> 2 </span>
				<span>
					<span class="font-semibold text-theme">If Windows SmartScreen appears</span>, click
					<span class="font-medium text-theme">More info → Run anyway</span>. The app is open source
					and not yet code-signed, so SmartScreen warns about an unknown publisher.
				</span>
			</li>
			<li class="flex gap-3">
				<span class="mt-0.5 w-4 shrink-0 font-mono text-xs text-neutral-900"> 3 </span>
				<span>
					<span class="font-semibold text-theme">Launch Splitwave</span> from the Start menu.
				</span>
			</li>
		</ol>

		<div class="flex flex-col gap-2 rounded-xl border border-dashed border-amber-500/50 px-4 py-3">
			<h3 class="inline-flex items-center gap-2 text-sm font-semibold text-theme">
				<Icons.Warning class="size-4 text-amber-500" />
				Why does SmartScreen warn me?
			</h3>
			<p class="text-sm leading-relaxed text-neutral-1000">
				Splitwave is an open-source app distributed outside the Microsoft Store and is not yet
				signed with a code-signing certificate. Windows SmartScreen shows an “unknown publisher”
				warning for apps it hasn't seen before. Choosing
				<span class="font-medium text-theme">More info → Run anyway</span> lets the installer run. It
				does not modify anything else on your system.
			</p>
		</div>
	</Section>
</div>
