<script lang="ts">
	import { openUrl } from '@tauri-apps/plugin-opener';
	import { modalManager, type ModalBaseProps } from '$lib/modules/overlay/modal';
	import { getCachedAppInfo, loadAppInfo } from '$lib/modules/app_info';
	import { t } from '$lib/modules/i18n';

	let { modalId }: ModalBaseProps = $props();

	const REPO = 'Horuse/Splitwave';
	let info = $derived(getCachedAppInfo());

	$effect(() => {
		if (!info) {
			loadAppInfo().catch(() => {});
		}
	});

	const links = [
		{ label: 'about.website', url: 'https://splitwave.app/' },
		{ label: 'about.github', url: `https://github.com/${REPO}` },
		{ label: 'about.issues', url: `https://github.com/${REPO}/issues` },
		{ label: 'about.discussions', url: `https://github.com/${REPO}/discussions` }
	];

	async function open(url: string) {
		try {
			await openUrl(url);
		} catch {}
	}

	function close() {
		modalManager.close(modalId);
	}
</script>

<div class="flex flex-col items-center gap-5 px-6 pt-2 pb-6">
	<div class="flex flex-col items-center gap-3">
		<img src="/logo.png" alt="Splitwave" class="size-16" />
		<div class="flex flex-col items-center gap-1.5">
			<h1 class="text-xl font-semibold text-theme">Splitwave</h1>
			<span class="rounded-full bg-neutral-200 px-2.5 py-0.5 font-mono text-[11px] text-neutral-1000 tabular-nums">
				v{info?.appVersion ?? '?'}
			</span>
		</div>
	</div>

	<p class="max-w-md text-center text-sm leading-relaxed text-neutral-900">
		{t('about.description')}
	</p>

	<div class="grid w-full grid-cols-2 gap-2">
		{#each links as link (link.url)}
			<button
				type="button"
				class="rounded-xl border border-neutral-400 bg-neutral-100 px-4 py-2 text-xs font-medium text-neutral-1000 transition-colors hover:bg-neutral-200 hover:text-theme"
				onclick={() => open(link.url)}>
				{t(link.label)}
			</button>
		{/each}
	</div>

	<button type="button" class="button-main primary rounded-lg" onclick={close}>{t('common.close')}</button>

	<div class="flex flex-col items-center gap-1 text-center text-[10px] text-neutral-900">
		<p>
			{t('about.builtWith')}
			<button type="button" class="underline hover:text-neutral-1000" onclick={() => open(`https://github.com/${REPO}/blob/main/LICENSE`)}
				>{t('about.license')}</button>
		</p>
		<a href="mailto:support@splitwave.app" class="underline hover:text-neutral-1000"> support@splitwave.app </a>
	</div>
</div>
