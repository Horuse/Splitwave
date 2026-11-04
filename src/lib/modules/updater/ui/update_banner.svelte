<script lang="ts">
	import { updaterStore } from '../stores.svelte';
	import { installUpdate, skipVersion } from '../methods';
	import { ModalShell } from '$lib/modules/overlay/ui';
	import CopyButton from '$lib/components/copy_button.svelte';
	import Markdown from '$lib/components/markdown.svelte';
	import { Checkmark } from '$lib/components/icons';
	import { getCachedAppInfo, loadAppInfo } from '$lib/modules/app_info';
	import { formatBytes } from '$lib/components/format';
	import { t } from '$lib/modules/i18n';

	let info = $derived(getCachedAppInfo());

	$effect(() => {
		if (!info) {
			loadAppInfo().catch(() => {});
		}
	});

	let s = $derived(updaterStore.state);

	let title = $derived.by(() => {
		if (s.phase === 'up_to_date') return t('updater.upToDate');
		if (s.phase === 'available') return t('updater.available');
		if (s.phase === 'downloading') return t('updater.downloading');
		if (s.phase === 'installing') return t('updater.installing');
		if (s.phase === 'unsupported') return t('updater.unsupported');
		return t('updater.failed');
	});

	let titleClass = $derived(s.phase === 'error' ? 'text-sm font-semibold text-red-500' : 'text-sm font-semibold text-theme');

	function progressPct(): number {
		if (s.phase !== 'downloading' || !s.total || s.total === 0) return 0;
		return Math.min(100, Math.round((s.downloaded / s.total) * 100));
	}

	function dismiss() {
		updaterStore.state = { phase: 'idle' };
	}

	function onSkip() {
		if (s.phase !== 'available') return;
		skipVersion(s.update.version);
	}
</script>

{#if s.phase === 'up_to_date' || s.phase === 'available' || s.phase === 'downloading' || s.phase === 'installing' || s.phase === 'unsupported' || s.phase === 'error'}
	<ModalShell {title} {titleClass} canClose={s.phase !== 'installing'} onClose={dismiss}>
		{#snippet badge()}
			{#if s.phase === 'available' || s.phase === 'downloading'}
				<div class="flex items-center gap-1.5">
					<span class="rounded-md bg-neutral-200 px-2 py-0.5 font-mono text-[10px] text-neutral-1000">
						v{s.update.version}
					</span>
					{#if /-(rc|beta|alpha)/i.test(s.update.version)}
						<span class="rounded-md bg-amber-500/20 px-1.5 py-0.5 font-sans text-[10px] font-semibold text-amber-600 dark:text-amber-400">
							{t('updater.preRelease')}
						</span>
					{/if}
				</div>
			{/if}
		{/snippet}

		<div class="px-5 py-4">
			{#if s.phase === 'up_to_date'}
				<div class="flex flex-col items-center gap-3 py-2">
					<div class="flex size-11 items-center justify-center rounded-full bg-emerald-500/15 text-emerald-600 dark:text-emerald-400">
						<Checkmark class="size-5" />
					</div>
					<div class="flex flex-col items-center gap-1.5">
						<p class="text-lg font-semibold text-theme">{t('updater.latestBody')}</p>
						<span class="rounded-full bg-neutral-200 px-2.5 py-0.5 font-mono text-[11px] text-neutral-1000 tabular-nums">
							v{info?.appVersion ?? '?'}
						</span>
					</div>
					<p class="text-xs text-neutral-900">{t('updater.checksOnLaunch')}</p>
				</div>
			{:else if s.phase === 'available'}
				<p class="mb-3 text-xs text-neutral-900">{t('updater.readyBody')}</p>
				{#if s.notes}
					<Markdown
						source={s.notes}
						class="max-h-60 overflow-auto rounded-lg border border-neutral-300 bg-neutral-200 p-3 text-xs leading-relaxed text-neutral-1100" />
				{/if}
			{:else if s.phase === 'downloading'}
				<div class="flex flex-col gap-3">
					<div class="flex items-baseline justify-between">
						<span class="font-mono text-3xl text-theme tabular-nums">
							{s.total ? `${progressPct()}%` : formatBytes(s.downloaded)}
						</span>
						<span class="font-mono text-xs text-neutral-900 tabular-nums">
							{s.total ? t('updater.of', { done: formatBytes(s.downloaded), total: formatBytes(s.total) }) : t('updater.sizeUnknown')}
						</span>
					</div>

					<div class="h-2.5 overflow-hidden rounded-full bg-neutral-300">
						{#if s.total}
							<div class="h-full rounded-full bg-emerald-500 transition-[width] duration-300 ease-out" style="width: {progressPct()}%;"></div>
						{:else}
							<!-- The server sent no content length, so completion cannot be shown. -->
							<div class="h-full w-1/3 animate-pulse rounded-full bg-emerald-500"></div>
						{/if}
					</div>

					<p class="text-xs text-neutral-900">{t('updater.keepOpen')}</p>
				</div>
			{:else if s.phase === 'installing'}
				<p class="text-xs text-neutral-900">{t('updater.finalizing')}</p>
			{:else if s.phase === 'unsupported'}
				<p class="text-xs text-neutral-900">{s.message}</p>
			{:else if s.phase === 'error'}
				<p class="mb-3 text-xs text-neutral-900">{t('updater.checkFailed')}</p>
				<pre
					class="max-h-60 overflow-auto rounded-lg border border-neutral-300 bg-neutral-200 p-3 font-mono text-[11px] leading-relaxed break-words whitespace-pre-wrap text-neutral-1100">{s.message}</pre>
			{/if}
		</div>

		{#snippet footer()}
			{#if s.phase === 'up_to_date'}
				<button type="button" class="button-main primary rounded-lg" onclick={dismiss}> {t('common.ok')} </button>
			{:else if s.phase === 'available'}
				<button type="button" class="button-main primary rounded-lg" onclick={onSkip}> {t('updater.skipVersion')} </button>
				<button type="button" class="button-main primary rounded-lg" onclick={dismiss}> {t('common.later')} </button>
				<button type="button" class="button-main green rounded-lg" onclick={installUpdate}> {t('updater.installRestart')} </button>
			{:else if s.phase === 'error'}
				<button type="button" class="button-main primary rounded-lg" onclick={dismiss}> {t('common.dismiss')} </button>
				<CopyButton text={s.message} label={t('updater.copyError')} class="button-main red gap-3 rounded-lg" />
			{:else if s.phase === 'unsupported'}
				<button type="button" class="button-main primary rounded-lg" onclick={dismiss}> {t('common.ok')} </button>
			{/if}
		{/snippet}
	</ModalShell>
{/if}
