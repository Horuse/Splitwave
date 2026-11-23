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
		iconColor: string;
		asset: Asset;
	}

	let { release }: { release: Release | null } = $props();

	// Matches the platform accent used by the selector on /download.
	const ACTIVE = 'border-amber-500/50 mix-bg-amber-500 !text-amber-400';

	const RELEASE = $derived(release?.url ?? 'https://github.com/Horuse/Splitwave/releases');

	const builds = $derived(
		[
			{
				id: 'appimage',
				label: 'AppImage',
				hint: 'Universal — runs on most distributions',
				icon: Icons.AppImage,
				iconColor: 'text-[#739FB9]',
				asset: release && pickAsset(release.assets, '.AppImage')
			},
			{
				id: 'deb',
				label: '.deb',
				hint: 'Debian, Ubuntu, Mint, Pop!_OS',
				icon: Icons.Debian,
				iconColor: 'text-[#A81D33]',
				asset: release && pickAsset(release.assets, '.deb')
			},
			{
				id: 'rpm',
				label: '.rpm',
				hint: 'Fedora, RHEL, openSUSE',
				icon: Icons.Fedora,
				iconColor: 'text-[#51A2DA]',
				asset: release && pickAsset(release.assets, '.rpm')
			}
		].filter((b): b is Build => !!b.asset)
	);

	let selected = $state('appimage');
	const current = $derived(builds.find((b) => b.id === selected) ?? builds[0]);

	const installCmd = $derived(
		!current
			? ''
			: selected === 'appimage'
				? `chmod +x ${current.asset.name}\n./${current.asset.name}`
				: selected === 'deb'
					? `sudo apt install ./${current.asset.name}`
					: `sudo dnf install ./${current.asset.name}`
	);

	let copied = $state(false);
	async function copyCommand() {
		try {
			await navigator.clipboard.writeText(installCmd);
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
					GitHub could not be reached. Grab the latest build from
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
						<build.icon class={['size-4', build.iconColor]} />
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
					64-bit x86_64. Requires a PipeWire audio session (default on most current distributions).
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
		{#if !current}
			<p class="text-sm text-neutral-1000">Instructions appear once the download list loads.</p>
		{:else if selected === 'appimage'}
			<ol class="flex flex-col gap-5 text-sm leading-relaxed text-neutral-1000">
				<li class="flex gap-3">
					<span class="mt-0.5 w-4 shrink-0 font-mono text-xs text-neutral-900"> 1 </span>
					<span>
						<span class="font-semibold text-theme">Download</span> the
						<span class="font-medium text-theme">.AppImage</span> above.
					</span>
				</li>
				<li class="flex gap-3">
					<span class="mt-0.5 w-4 shrink-0 font-mono text-xs text-neutral-900"> 2 </span>
					<span class="flex min-w-0 flex-1 flex-col gap-2">
						<span>
							<span class="font-semibold text-theme">Make it executable and run it.</span>
							In the download folder:
						</span>
						<span
							class="flex flex-wrap items-center gap-2 rounded-xl border border-dashed border-neutral-400 p-3 pl-5"
						>
							<code class="flex-1 font-mono text-xs whitespace-pre text-theme select-all">
								{installCmd}
							</code>
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
						Or just make it executable in your file manager (Properties →
						<span class="font-medium text-theme">Allow executing as program</span>) and double-click
						it.
					</span>
				</li>
			</ol>
		{:else}
			<ol class="flex flex-col gap-5 text-sm leading-relaxed text-neutral-1000">
				<li class="flex gap-3">
					<span class="mt-0.5 w-4 shrink-0 font-mono text-xs text-neutral-900"> 1 </span>
					<span>
						<span class="font-semibold text-theme">Download</span> the
						<span class="font-medium text-theme">{current.label}</span> package above.
					</span>
				</li>
				<li class="flex gap-3">
					<span class="mt-0.5 w-4 shrink-0 font-mono text-xs text-neutral-900"> 2 </span>
					<span class="flex min-w-0 flex-1 flex-col gap-2">
						<span>
							<span class="font-semibold text-theme">Install it.</span>
							In the download folder:
						</span>
						<span
							class="flex flex-wrap items-center gap-2 rounded-xl border border-dashed border-neutral-400 p-3 pl-5"
						>
							<code class="flex-1 font-mono text-xs text-theme select-all">{installCmd}</code>
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
						<span class="font-semibold text-theme">Launch Splitwave</span> from your app menu or run
						<span class="font-mono text-xs">splitwave</span>.
					</span>
				</li>
			</ol>
		{/if}

		<div class="flex flex-col gap-3 rounded-xl border border-dashed border-neutral-400 px-4 py-3">
			<h3 class="inline-flex items-center gap-2 text-sm font-semibold text-theme">
				<Icons.Info class="size-4 text-blue-400" />
				Which one should I pick?
			</h3>
			<p class="text-sm leading-relaxed text-neutral-1000">
				The <span class="font-medium text-theme">AppImage</span> is the easiest — one self-contained
				file that runs on almost any distribution, no installation needed. Use the
				<span class="font-mono text-xs">.deb</span> on Debian/Ubuntu-based systems or the
				<span class="font-mono text-xs">.rpm</span> on Fedora/RHEL/openSUSE if you prefer your package
				manager to track the app and pull dependencies automatically.
			</p>
			<ul class="flex flex-wrap gap-2 text-xs text-neutral-1000">
				<li
					class="inline-flex items-center gap-1.5 rounded-full border border-neutral-300 px-3 py-1"
				>
					<Icons.Debian class="size-3.5 text-[#A81D33]" />
					Debian
				</li>
				<li
					class="inline-flex items-center gap-1.5 rounded-full border border-neutral-300 px-3 py-1"
				>
					<Icons.Ubuntu class="size-3.5 text-[#E95420]" />
					Ubuntu
				</li>
				<li
					class="inline-flex items-center gap-1.5 rounded-full border border-neutral-300 px-3 py-1"
				>
					<Icons.Fedora class="size-3.5 text-[#51A2DA]" />
					Fedora
				</li>
				<li
					class="inline-flex items-center gap-1.5 rounded-full border border-neutral-300 px-3 py-1"
				>
					<Icons.RedHat class="size-3.5 text-[#EE0000]" />
					RHEL
				</li>
				<li
					class="inline-flex items-center gap-1.5 rounded-full border border-neutral-300 px-3 py-1"
				>
					<Icons.OpenSuse class="size-3.5 text-[#73BA25]" />
					openSUSE
				</li>
			</ul>
		</div>
	</Section>
</div>
