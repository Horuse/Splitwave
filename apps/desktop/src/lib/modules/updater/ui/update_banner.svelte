<script lang="ts">
	import { Button, Modal } from '@splitwave/ui';
	import { updaterStore } from '../stores.svelte';
	import { installUpdate, skipVersion } from '../methods';
	import CopyButton from '$lib/components/copy_button.svelte';
	import Markdown from '$lib/components/markdown.svelte';
	import Checkmark from '~icons/fluent/checkmark-16-filled';
	import Download from '~icons/fluent/arrow-download-16-filled';
	import ErrorCircle from '~icons/fluent/error-circle-16-filled';
	import ArrowRight from '~icons/fluent/arrow-right-12-filled';
	import { getCachedAppInfo, loadAppInfo } from '$lib/modules/app_info';
	import { formatBytes } from '@splitwave/nodes';

	let info = $derived(getCachedAppInfo());

	$effect(() => {
		if (!info) {
			loadAppInfo().catch(() => {});
		}
	});

	let s = $derived(updaterStore.state);

	let title = $derived.by(() => {
		if (s.phase === 'up_to_date') return 'Up to date';
		if (s.phase === 'available') return 'Update available';
		if (s.phase === 'downloading') return 'Downloading update';
		if (s.phase === 'installing') return 'Installing update';
		if (s.phase === 'unsupported') return 'No build available';
		return 'Update failed';
	});

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
	<Modal {title} canClose={s.phase !== 'installing'} onClose={dismiss}>
		{#snippet icon()}
			{#if s.phase === 'available' || s.phase === 'downloading' || s.phase === 'installing'}
				<div class="flex size-9 shrink-0 items-center justify-center rounded-xl bg-emerald-500/15 text-emerald-600 dark:text-emerald-400">
					<Download class="size-4.5" />
				</div>
			{:else if s.phase === 'error'}
				<div class="flex size-9 shrink-0 items-center justify-center rounded-xl bg-red-500/15 text-red-500">
					<ErrorCircle class="size-4.5" />
				</div>
			{/if}
		{/snippet}
		{#snippet badge()}
			{#if (s.phase === 'available' || s.phase === 'downloading') && /-(rc|beta|alpha)/i.test(s.update.version)}
				<span class="rounded-md bg-amber-500/15 px-1.5 py-0.5 text-[10px] font-semibold text-amber-600 dark:text-amber-400">Pre-release</span>
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
				<div class="mb-3 flex items-center gap-2 font-mono text-xs tabular-nums">
					{#if info}
						<span class="text-neutral-800">v{info.appVersion}</span>
						<ArrowRight class="size-3 text-neutral-700" />
					{/if}
					<span class="rounded-md bg-emerald-500/15 px-1.5 py-0.5 font-semibold text-emerald-600 dark:text-emerald-400">v{s.update.version}</span>
				</div>
				{#if s.notes}
					<Markdown
						source={s.notes}
						class="max-h-72 overflow-auto rounded-lg bg-(--field-bg,var(--lv-1)) px-4 py-3 text-xs leading-relaxed text-neutral-1100" />
				{/if}
				<p class="mt-3 text-[11px] text-neutral-800">Your work is saved before the app restarts.</p>
			{:else if s.phase === 'downloading'}
				<div class="flex flex-col gap-3">
					<div class="flex items-baseline justify-between">
						<span class="font-mono text-3xl text-theme tabular-nums">
							{s.total ? `${progressPct()}%` : formatBytes(s.downloaded)}
						</span>
						<span class="font-mono text-xs text-neutral-900 tabular-nums">
							{s.total ? `${formatBytes(s.downloaded)} of ${formatBytes(s.total)}` : 'Size unknown'}
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
			{:else if s.phase === 'unsupported'}
				<p class="text-xs text-neutral-900">{s.message}</p>
			{:else if s.phase === 'error'}
				<p class="mb-3 text-xs text-neutral-900">Couldn't check for updates.</p>
				<pre
					class="max-h-60 overflow-auto rounded-lg bg-(--field-bg,var(--lv-1)) px-3 py-2.5 font-mono text-[11px] leading-relaxed break-words whitespace-pre-wrap text-neutral-1100">{s.message}</pre>
			{/if}
		</div>

		{#snippet footer()}
			{#if s.phase === 'up_to_date'}
				<Button onclick={dismiss}>OK</Button>
			{:else if s.phase === 'available'}
				<Button variant="ghost" class="mr-auto" onclick={onSkip}>Skip this version</Button>
				<Button onclick={dismiss}>Later</Button>
				<Button tone="success" variant="solid" type="button" onclick={installUpdate}>Install &amp; restart</Button>
			{:else if s.phase === 'error'}
				<Button variant="ghost" class="mr-auto" onclick={dismiss}>Dismiss</Button>
				<CopyButton text={s.message} label="Copy error" />
			{:else if s.phase === 'unsupported'}
				<Button onclick={dismiss}>OK</Button>
			{/if}
		{/snippet}
	</Modal>
{/if}
