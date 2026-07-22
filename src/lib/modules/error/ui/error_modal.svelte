<script lang="ts">
	import { openUrl } from '@tauri-apps/plugin-opener';
	import { errorStore } from '../stores.svelte';
	import { formatAppInfo, getCachedAppInfo } from '$lib/modules/app_info';
	import { ModalShell } from '$lib/modules/overlay/ui/modal';
	import CopyButton from '$lib/components/copy_button.svelte';

	const REPO = 'Horuse/Splitwave';

	let current = $derived(errorStore.current);

	function buildDetails(): string {
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
		const title = `[crash] ${current.message.split('\n')[0].slice(0, 80)}`;
		const body = buildDetails();
		const url = `https://github.com/${REPO}/issues/new?template=crash.yml&title=${encodeURIComponent(title)}&body=${encodeURIComponent(body)}`;
		try {
			await openUrl(url);
		} catch {
		}
	}

	function dismiss() {
		errorStore.dismiss();
	}

	function sourceLabel(s: NonNullable<typeof current>['source']): string {
		switch (s) {
			case 'rustPanic':
				return 'Rust panic';
			case 'jsError':
				return 'JS error';
			case 'unhandledRejection':
				return 'Unhandled promise rejection';
		}
	}
</script>

{#if current}
	<ModalShell
		title={current.previousRun ? 'Splitwave crashed last time' : 'Something went wrong'}
		titleClass={current.previousRun
			? 'text-sm font-semibold text-amber-500'
			: 'text-sm font-semibold text-red-500'}
		onClose={dismiss}
	>
		{#snippet badge()}
			<span class="rounded-md bg-neutral-200 px-2 py-0.5 font-mono text-[10px] text-neutral-1000">
				{sourceLabel(current.source)}
			</span>
		{/snippet}

		<div class="flex flex-col gap-3 px-5 py-4">
			<p class="text-xs text-neutral-900">
				{#if current.previousRun}
					The app closed unexpectedly during your previous session. Reporting this helps us fix it.
				{:else}
					Please report this so we can fix it.
				{/if}
			</p>
			<pre class="max-h-60 overflow-auto rounded-lg border border-neutral-300 bg-neutral-200 p-3 font-mono text-[11px] leading-relaxed whitespace-pre-wrap break-words text-neutral-1100">{current.message}</pre>
			{#if current.stack}
				<details class="group">
					<summary class="cursor-pointer text-[11px] text-neutral-900 transition-colors hover:text-neutral-1100">Stack trace</summary>
					<pre class="mt-2 max-h-60 overflow-auto rounded-lg border border-neutral-300 bg-neutral-200 p-3 font-mono text-[10px] leading-relaxed whitespace-pre-wrap break-words text-neutral-1000">{current.stack}</pre>
				</details>
			{/if}
		</div>

		{#snippet footer()}
			<button type="button" class="button-main primary rounded-lg" onclick={dismiss}>
				Dismiss
			</button>
			<CopyButton
				text={buildDetails}
				label="Copy details"
				class="button-main primary gap-3 rounded-lg"
			/>
			<button type="button" class="button-main red rounded-lg" onclick={reportOnGitHub}>
				Report on GitHub
			</button>
		{/snippet}
	</ModalShell>
{/if}
