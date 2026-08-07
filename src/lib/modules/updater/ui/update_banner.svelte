<script lang="ts">
	import { updaterStore } from '../stores.svelte';
	import { installUpdate, skipVersion } from '../methods';
	import { ModalShell } from '$lib/modules/overlay/ui';
	import CopyButton from '$lib/components/copy_button.svelte';
	import Markdown from '$lib/components/markdown.svelte';
	import { Checkmark } from '$lib/components/icons';
	import { getCachedAppInfo } from '$lib/modules/app_info';

	const info = getCachedAppInfo();

	let s = $derived(updaterStore.state);

	let title = $derived.by(() => {
		if (s.phase === 'up_to_date') return 'Up to date';
		if (s.phase === 'available') return 'Update available';
		if (s.phase === 'downloading') return 'Downloading update';
		if (s.phase === 'installing') return 'Installing update';
		return 'Update failed';
	});

	let titleClass = $derived(s.phase === 'error' ? 'text-sm font-semibold text-red-500' : 'text-sm font-semibold text-theme');

	function progressPct(): number {
		if (s.phase !== 'downloading' || !s.total || s.total === 0) return 0;
		return Math.min(100, Math.round((s.downloaded / s.total) * 100));
	}

	function mb(bytes: number): string {
		return `${(bytes / 1_000_000).toFixed(1)} MB`;
	}

	function dismiss() {
		updaterStore.state = { phase: 'idle' };
	}

	function onSkip() {
		if (s.phase !== 'available') return;
		skipVersion(s.update.version);
	}
</script>

{#if s.phase === 'up_to_date' || s.phase === 'available' || s.phase === 'downloading' || s.phase === 'installing' || s.phase === 'error'}
	<ModalShell {title} {titleClass} canClose={s.phase !== 'installing'} onClose={dismiss}>
		{#snippet badge()}
			{#if s.phase === 'available' || s.phase === 'downloading'}
				<span class="rounded-md bg-neutral-200 px-2 py-0.5 font-mono text-[10px] text-neutral-1000">
					v{s.update.version}
				</span>
			{/if}
		{/snippet}

		<div class="px-5 py-4">
			{#if s.phase === 'up_to_date'}
				<div class="flex flex-col items-center gap-3 py-2">
					<div class="flex size-11 items-center justify-center rounded-full bg-emerald-500/15 text-emerald-600 dark:text-emerald-400">
						<Checkmark class="size-5" />
					</div>
					<div class="flex flex-col items-center gap-1.5">
						<p class="text-lg font-semibold text-theme">You're on the latest version</p>
						<span class="rounded-full bg-neutral-200 px-2.5 py-0.5 font-mono text-[11px] text-neutral-1000 tabular-nums">
							v{info?.appVersion ?? '?'}
						</span>
					</div>
					<p class="text-xs text-neutral-900">Splitwave checks for updates on launch.</p>
				</div>
			{:else if s.phase === 'available'}
				<p class="mb-3 text-xs text-neutral-900">A new version is ready to install. Your work will be saved before restarting.</p>
				{#if s.notes}
					<Markdown
						source={s.notes}
						class="max-h-60 overflow-auto rounded-lg border border-neutral-300 bg-neutral-200 p-3 text-xs leading-relaxed text-neutral-1100" />
				{/if}
			{:else if s.phase === 'downloading'}
				<div class="flex flex-col gap-3">
					<div class="flex items-baseline justify-between">
						<span class="font-mono text-3xl text-theme tabular-nums">
							{s.total ? `${progressPct()}%` : mb(s.downloaded)}
						</span>
						<span class="font-mono text-xs text-neutral-900 tabular-nums">
							{s.total ? `${mb(s.downloaded)} of ${mb(s.total)}` : 'Size unknown'}
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

					<p class="text-xs text-neutral-900">Keep the app open. It will restart itself once the update is installed.</p>
				</div>
			{:else if s.phase === 'installing'}
				<p class="text-xs text-neutral-900">Finalizing. The app will restart in a moment.</p>
			{:else if s.phase === 'error'}
				<p class="mb-3 text-xs text-neutral-900">Couldn't check for updates.</p>
				<pre
					class="max-h-60 overflow-auto rounded-lg border border-neutral-300 bg-neutral-200 p-3 font-mono text-[11px] leading-relaxed break-words whitespace-pre-wrap text-neutral-1100">{s.message}</pre>
			{/if}
		</div>

		{#snippet footer()}
			{#if s.phase === 'up_to_date'}
				<button type="button" class="button-main primary rounded-lg" onclick={dismiss}> OK </button>
			{:else if s.phase === 'available'}
				<button type="button" class="button-main primary rounded-lg" onclick={onSkip}> Skip this version </button>
				<button type="button" class="button-main primary rounded-lg" onclick={dismiss}> Later </button>
				<button type="button" class="button-main green rounded-lg" onclick={installUpdate}> Install &amp; restart </button>
			{:else if s.phase === 'error'}
				<button type="button" class="button-main primary rounded-lg" onclick={dismiss}> Dismiss </button>
				<CopyButton text={s.message} label="Copy error" class="button-main red gap-3 rounded-lg" />
			{/if}
		{/snippet}
	</ModalShell>
{/if}
