<script lang="ts">
	import Checksum from '#lib/components/Checksum.svelte';
	import Section from '#lib/components/Section.svelte';
	import * as Icons from '#lib/components/icons/index.js';
	import { formatSize, pickAsset, type Asset, type Release } from '#lib/releases.js';

	interface Build {
		id: string;
		label: string;
		hint: string;
		icon: Icons.IconComponent;
		asset: Asset;
	}

	let { release }: { release: Release | null } = $props();

	// Matches the platform accent used by the selector on /download.
	const ACTIVE = 'border-sky-500/50 mix-bg-sky-500 !text-sky-400';

	const RELEASE = $derived(release?.url ?? 'https://github.com/Horuse/Splitwave/releases');

	const builds = $derived(
		[
			{
				id: 'arm',
				label: 'Apple Silicon',
				hint: 'M1, M2, M3, M4, M5 - and newer',
				icon: Icons.Laptop,
				asset: release && pickAsset(release.assets, '_aarch64.dmg')
			},
			{
				id: 'intel',
				label: 'Intel',
				hint: 'Macs with an Intel processor',
				icon: Icons.Desktop,
				asset: release && pickAsset(release.assets, '_x64.dmg')
			}
		].filter((b): b is Build => !!b.asset)
	);

	let selected = $state('arm');
	const current = $derived(builds.find((b) => b.id === selected) ?? builds[0]);

	const XATTR = 'xattr -cr /Applications/Splitwave.app';
	let copied = $state(false);

	async function copyCommand() {
		try {
			await navigator.clipboard.writeText(XATTR);
			copied = true;
			setTimeout(() => (copied = false), 2000);
		} catch {
			copied = false;
		}
	}
</script>

<div class="flex flex-col gap-12">
	<Section label="Download">
		{#if !current}
			<div
				class="flex flex-col gap-2 rounded-xl border border-dashed border-amber-500/50 px-4 py-3"
			>
				<p class="inline-flex items-center gap-2 font-semibold text-theme">
					<Icons.Warning class="size-4 text-amber-500" />
					Downloads are unavailable right now.
				</p>
				<p class="text-sm text-neutral-1000">
					GitHub could not be reached. Grab the latest .dmg from
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
				{#each builds as build (build.id)}
					<button
						type="button"
						aria-pressed={selected === build.id}
						class={['btn-tab', selected === build.id && ACTIVE]}
						onclick={() => (selected = build.id)}
					>
						<build.icon class="size-4" />
						{build.label}
					</button>
				{/each}
			</div>

			<div class="flex flex-col gap-4">
				<p class="text-sm text-neutral-1000">{current.hint}</p>
				<div class="flex flex-wrap items-center gap-3">
					<a href={current.asset.url} class="btn-primary break-all">
						<Icons.ArrowDownload class="size-4 shrink-0" />
						Download {current.asset.name}
					</a>
					<span class="inline-flex items-center gap-1.5 text-xs text-neutral-900">
						<Icons.HardDrive class="size-3.5" />
						{formatSize(current.asset.size)}
					</span>
					{#if current.asset.sha256}
						<Checksum sha256={current.asset.sha256} />
					{/if}
				</div>
				<p class="text-xs text-neutral-900">
					Not sure which one? Click the Apple menu → About This Mac. “Apple M…” means Apple Silicon,
					“Intel” means Intel.
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
					<span class="font-semibold text-theme">Open the .dmg</span> and drag
					<span class="font-medium text-theme">Splitwave</span> into your
					<span class="font-medium text-theme">Applications</span> folder.
				</span>
			</li>
			<li class="flex gap-3">
				<span class="mt-0.5 w-4 shrink-0 font-mono text-xs text-neutral-900"> 2 </span>
				<span class="flex min-w-0 flex-1 flex-col gap-2">
					<span>
						<span class="font-semibold text-theme">Remove the quarantine flag.</span>
						Open Terminal and run:
					</span>
					<span
						class="flex flex-wrap items-center gap-2 rounded-xl border border-dashed border-neutral-400 p-3 pl-5"
					>
						<code class="flex-1 font-mono text-xs text-theme select-all">{XATTR}</code>
						<button type="button" class="btn-ghost" onclick={copyCommand}>
							{#if copied}
								<Icons.Checkmark class="size-3.5" />
								Copied
							{:else}
								<Icons.Copy class="size-3.5" />
								Copy
							{/if}
						</button>
					</span>
				</span>
			</li>
			<li class="flex gap-3">
				<span class="mt-0.5 w-4 shrink-0 font-mono text-xs text-neutral-900"> 3 </span>
				<span>
					<span class="font-semibold text-theme">Launch Splitwave</span> from Applications.
				</span>
			</li>
		</ol>

		<div class="flex flex-col gap-2 rounded-xl border border-dashed border-amber-500/50 px-4 py-3">
			<h3 class="inline-flex items-center gap-2 text-sm font-semibold text-theme">
				<Icons.Warning class="size-4 text-amber-500" />
				Why is this needed?
			</h3>
			<p class="text-sm leading-relaxed text-neutral-1000">
				Splitwave is an open-source app distributed outside the Mac App Store and is not yet
				notarized by Apple. When you download a file from the internet, macOS attaches a <span
					class="font-mono text-xs">com.apple.quarantine</span
				>
				attribute to it. Because the app isn't notarized, Gatekeeper blocks it with a “Splitwave is damaged
				and can't be opened” or “unidentified developer” message. The
				<span class="font-mono text-xs">xattr -cr</span>
				command clears those extended attributes (<span class="font-mono text-xs">-c</span> clears,
				<span class="font-mono text-xs">-r</span> applies recursively), which removes the quarantine flag
				so macOS lets the app run. It does not modify the app itself.
			</p>
		</div>
	</Section>
</div>
