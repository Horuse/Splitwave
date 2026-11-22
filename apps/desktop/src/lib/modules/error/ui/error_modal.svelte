<script lang="ts">
	import { Button, Modal } from '@splitwave/ui';
	import { openUrl } from '@tauri-apps/plugin-opener';
	import { errorStore } from '../stores.svelte';
	import { audioStore } from '$lib/modules/audio/stores.svelte';
	import { formatAppInfo, getCachedAppInfo, loadAppInfo } from '$lib/modules/app_info';
	import CopyButton from '$lib/components/copy_button.svelte';
	import Warning from '~icons/fluent/warning-16-filled';
	import ErrorCircle from '~icons/fluent/error-circle-16-filled';
	import ShieldError from '~icons/fluent/shield-error-16-filled';
	import Chevron from '~icons/fluent/chevron-right-12-filled';

	const REPO = 'Horuse/Splitwave';
	// Longer issue URLs are cut off or rejected, so the GitHub link carries only
	// the newest log lines that fit; Copy details carries all of it.
	const MAX_URL_LENGTH = 8000;

	let current = $derived(errorStore.current);

	$effect(() => {
		if (current) loadAppInfo().catch(() => {});
	});

	function buildDetails(log = current?.logs): string {
		if (!current) return '';
		const info = getCachedAppInfo();
		const lines = [
			'## Diagnostic info',
			`- **App:** Splitwave ${info?.appVersion ?? '?'} (tauri ${info?.tauriVersion ?? '?'})`,
			`- **OS:** ${info ? formatAppInfo(info) : navigator.userAgent}`,
			`- **Source:** ${current.source}${current.thread ? ` (thread: ${current.thread})` : ''}`,
			`- **Time:** ${new Date(current.at).toISOString()}`,
			'',
			'**Message:**',
			'```',
			current.message,
			'```'
		];
		if (current.stack) {
			lines.push('', '**Stack:**', '```', current.stack, '```');
		}
		if (log !== undefined) {
			lines.push('', '<details><summary>Session log</summary>', '', '```', log, '```', '', '</details>');
		}
		lines.push(
			'',
			'## Steps to reproduce',
			'<!-- What were you doing when this happened? Be specific. -->',
			'1. ',
			'2. ',
			'3. ',
			'',
			'## Expected behavior',
			'<!-- What did you expect to happen? -->',
			'',
			'## Additional context',
			'<!-- Screenshots, logs, related issues, anything else useful. -->',
			''
		);
		return lines.join('\n');
	}

	async function reportOnGitHub() {
		if (!current) return;
		await loadAppInfo().catch(() => {});
		const title = `[crash] ${current.message.split('\n')[0].slice(0, 80)}`;
		const issueUrl = (details: string) =>
			`https://github.com/${REPO}/issues/new?template=crash.yml&title=${encodeURIComponent(title)}&details=${encodeURIComponent(details)}`;
		const url = issueUrl(buildDetails(fittingLog(issueUrl(buildDetails('')).length)));
		try {
			await openUrl(url);
		} catch {}
	}

	/** Newest log lines whose encoded length fits next to a URL of `baseLength`. */
	function fittingLog(baseLength: number): string | undefined {
		if (!current?.logs) return undefined;
		const lines = current.logs.split('\n');
		let length = baseLength;
		let start = lines.length;
		while (start > 0) {
			length += encodeURIComponent(lines[start - 1] + '\n').length;
			if (length > MAX_URL_LENGTH) break;
			start -= 1;
		}
		if (start === lines.length) return undefined;
		return lines.slice(start).join('\n');
	}

	function dismiss() {
		errorStore.dismiss();
	}

	function sourceLabel(s: NonNullable<typeof current>['source']): string {
		switch (s) {
			case 'rustPanic':
				return 'Rust panic';
			case 'nativeCrash':
				return 'Native crash';
			case 'unexpectedExit':
				return 'Unexpected exit';
			case 'hang':
				return 'Not responding';
			case 'jsError':
				return 'JS error';
			case 'unhandledRejection':
				return 'Unhandled promise rejection';
		}
	}
</script>

{#snippet trace(label: string, text: string)}
	<details class="group border-t border-theme/5 first:border-t-0">
		<summary class="flex cursor-pointer list-none items-center gap-1.5 px-3 py-2 text-[11px] text-neutral-900 hover:text-neutral-1100">
			<Chevron class="size-3 group-open:rotate-90" />
			{label}
		</summary>
		<pre class="max-h-60 overflow-auto px-3 pb-3 font-mono text-[10px] leading-relaxed break-words whitespace-pre-wrap text-neutral-1000">{text}</pre>
	</details>
{/snippet}

{#if current}
	{@const safe = current.previousRun || audioStore.safeMode}
	<Modal
		title={current.previousRun ? 'Splitwave crashed last time' : 'Something went wrong'}
		description={current.previousRun ? 'The app closed unexpectedly during your previous session.' : 'Reporting this helps us fix it.'}
		onClose={dismiss}>
		{#snippet icon()}
			<div
				class={[
					'flex size-9 shrink-0 items-center justify-center rounded-xl',
					current.previousRun ? 'bg-amber-500/15 text-amber-600 dark:text-amber-400' : 'bg-red-500/15 text-red-500'
				]}>
				{#if current.previousRun}<Warning class="size-4.5" />{:else}<ErrorCircle class="size-4.5" />{/if}
			</div>
		{/snippet}
		{#snippet badge()}
			<span class="rounded-md bg-neutral-200 px-2 py-0.5 font-mono text-[10px] text-neutral-1000">{sourceLabel(current.source)}</span>
		{/snippet}

		<div class="flex flex-col gap-3 px-5 py-4">
			{#if safe}
				<div class="flex gap-2.5 rounded-lg bg-amber-500/10 px-3 py-2.5 text-amber-600 dark:text-amber-400">
					<ShieldError class="mt-px size-4 shrink-0" />
					<div class="flex flex-col gap-0.5">
						<span class="text-xs font-semibold">Safe Mode</span>
						<span class="text-[11px] opacity-90">
							Pipelines did not start on launch, to avoid a crash loop. Start them by hand when you are ready.
						</span>
					</div>
				</div>
			{/if}
			<div class="overflow-hidden rounded-lg bg-(--field-bg,var(--lv-1))">
				<pre
					class="max-h-40 overflow-auto px-3 py-2.5 font-mono text-[11px] leading-relaxed break-words whitespace-pre-wrap text-neutral-1100">{current.message}</pre>
				{#if current.stack || current.logs}
					<div class="border-t border-theme/5">
						{#if current.stack}{@render trace('Stack trace', current.stack)}{/if}
						{#if current.logs}{@render trace('Session log', current.logs)}{/if}
					</div>
				{/if}
			</div>
		</div>

		{#snippet footer()}
			<Button variant="ghost" class="mr-auto" onclick={dismiss}>Dismiss</Button>
			<CopyButton text={() => buildDetails()} label="Copy details" />
			<Button tone="danger" variant="solid" type="button" onclick={reportOnGitHub}>Report on GitHub</Button>
		{/snippet}
	</Modal>
{/if}
