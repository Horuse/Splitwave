<script lang="ts">
	import { onMount } from 'svelte';
	import * as Icons from '#lib/components/icons/index.js';
	import MacInstaller from '#lib/components/MacInstaller.svelte';
	import LinuxInstaller from '#lib/components/LinuxInstaller.svelte';
	import WindowsInstaller from '#lib/components/WindowsInstaller.svelte';
	import Support from '#lib/components/Support.svelte';
	import Section from '#lib/components/Section.svelte';
	import { formatDate } from '#lib/releases.js';

	let { data } = $props();

	type Os = 'mac' | 'linux' | 'windows';

	// Accents follow the node-category convention from the landing page: a colour
	// learned on one page still means the same platform here.
	const platforms = [
		{
			id: 'mac' as Os,
			label: 'macOS',
			requirement: 'macOS 13+ · Apple Silicon & Intel',
			icon: Icons.Apple,
			iconColor: 'text-theme',
			active: 'border-sky-500/50 mix-bg-sky-500 text-sky-400'
		},
		{
			id: 'linux' as Os,
			label: 'Linux',
			requirement: 'x86_64 · PipeWire',
			icon: Icons.Linux,
			iconColor: 'text-theme',
			active: 'border-amber-500/50 mix-bg-amber-500 text-amber-400'
		},
		{
			id: 'windows' as Os,
			label: 'Windows',
			requirement: 'Windows 10+ · 64-bit',
			icon: Icons.Windows,
			iconColor: '',
			active: 'border-blue-500/50 mix-bg-blue-500 text-blue-400'
		}
	];

	let os = $state<Os>('mac');
	let detected = $state(false);

	onMount(() => {
		const ua = navigator.userAgent;
		if (/Windows/.test(ua)) os = 'windows';
		else if (/Linux/.test(ua) && !/Android/.test(ua)) os = 'linux';
		else if (!/Mac/.test(ua)) return;
		detected = true;
	});

	function pick(next: Os) {
		os = next;
		detected = false;
	}
</script>

<div class="mx-auto flex max-w-4xl flex-col gap-6 px-6 py-16">
	<a
		href="/"
		class="inline-flex items-center gap-1.5 self-start text-sm text-neutral-900 transition-colors hover:text-theme"
	>
		<Icons.ArrowLeft class="size-4" />
		Back
	</a>

	<div class="mb-6 flex flex-col gap-2">
		<h1 class="text-3xl font-bold">Download Splitwave</h1>
		<p class="text-sm text-neutral-900">
			{#if data.release}
				Version {data.release.version}, released {formatDate(data.release.publishedAt)}.
				<a
					href="/changelog"
					class="text-theme underline decoration-neutral-500 underline-offset-2 transition-colors hover:decoration-theme"
				>
					What's new
				</a>
			{:else}
				Free and open source for macOS, Linux and Windows.
			{/if}
		</p>
	</div>

	<div class="flex flex-col gap-12">
		<Section label="Platform">
			<div class="grid gap-3 sm:grid-cols-3">
				{#each platforms as p (p.id)}
					<button
						type="button"
						onclick={() => pick(p.id)}
						aria-pressed={os === p.id}
						class={[
							'relative flex flex-col items-start gap-1 rounded-xl border px-4 py-3 text-left transition-colors duration-300',
							os === p.id
								? p.active
								: 'border-dashed border-neutral-400 text-neutral-1000 hover:border-neutral-600'
						]}
					>
						<span class="inline-flex items-center gap-2 font-semibold">
							<p.icon class={['size-4', p.iconColor]} />
							{p.label}
							{#if os === p.id && detected}
								<span class="text-xs font-normal text-neutral-900">· detected</span>
							{/if}
						</span>
						<span class="text-xs text-neutral-900">{p.requirement}</span>
					</button>
				{/each}
			</div>
		</Section>

		{#if os === 'mac'}
			<MacInstaller release={data.release} />
		{:else if os === 'linux'}
			<LinuxInstaller release={data.release} />
		{:else}
			<WindowsInstaller release={data.release} />
		{/if}

		<Section label="Support">
			<p class="text-sm text-neutral-1000">
				If you find Splitwave useful, consider supporting its development via Patreon or crypto.
				Thank you!
			</p>
			<Support />
		</Section>
	</div>
</div>
