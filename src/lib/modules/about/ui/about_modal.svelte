<script lang="ts">
	import { openUrl } from '@tauri-apps/plugin-opener';
	import { modalManager, type ModalBaseProps } from '$lib/modules/overlay/modal';
	import { getCachedAppInfo } from '$lib/modules/app_info';

	let { modalId }: ModalBaseProps = $props();

	const REPO = 'Horuse/Splitwave';
	const info = getCachedAppInfo();

	const links = [
		{ label: 'Website', url: 'https://splitwave.app/' },
		{ label: 'GitHub', url: `https://github.com/${REPO}` },
		{ label: 'Issues', url: `https://github.com/${REPO}/issues` },
		{ label: 'Discussions', url: `https://github.com/${REPO}/discussions` }
	];

	async function open(url: string) {
		try {
			await openUrl(url);
		} catch {
		}
	}

	function close() {
		modalManager.close(modalId);
	}
</script>

<div class="flex flex-col items-center gap-4 px-6 py-7">
	<img src="/logo.png" alt="Splitwave" class="h-20 w-20" />

	<div class="flex flex-col items-center gap-1">
		<h1 class="text-2xl font-bold text-neutral-1100">Splitwave</h1>
		<span class="rounded-full bg-neutral-200 px-2.5 py-0.5 font-mono text-[11px] tabular-nums text-neutral-1000">
			v{info?.appVersion ?? '?'}
		</span>
	</div>

	<p class="max-w-md text-center text-sm text-neutral-1000">
		Audio routing for macOS, Linux, and Windows. Build a node graph of inputs, effects, and outputs;
		the engine processes audio in real time and writes to files in any of six formats.
	</p>

	<div class="flex flex-wrap items-center justify-center gap-2 pt-2">
		{#each links as link (link.url)}
			<button
				type="button"
				class="button-header h-8 rounded-md px-4 text-xs"
				onclick={() => open(link.url)}
			>
				{link.label}
			</button>
		{/each}
	</div>

	<button type="button" class="button-main primary mt-2 rounded-lg" onclick={close}>
		Close
	</button>

	<p class="mt-2 text-center text-[10px] text-neutral-900">
		Built with Tauri, Svelte, Rust ·
		<button
			type="button"
			class="underline hover:text-neutral-1000"
			onclick={() => open(`https://github.com/${REPO}/blob/main/LICENSE`)}
		>MIT License</button>
	</p>

	<p class="text-center text-[10px] text-neutral-900">
		<a href="mailto:support@splitwave.app" class="underline hover:text-neutral-1000">support@splitwave.app</a>
	</p>
</div>
