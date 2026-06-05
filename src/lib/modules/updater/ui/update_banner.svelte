<script lang="ts">
	import { updaterStore } from '../stores.svelte';
	import { installUpdate, skipVersion } from '../methods';
	import { ModalShell } from '$lib/modules/overlay/ui';
	import CopyButton from '$lib/components/copy_button.svelte';

	let s = $derived(updaterStore.state);

	let title = $derived.by(() => {
		if (s.phase === 'up_to_date') return 'Up to date';
		if (s.phase === 'available') return 'Update available';
		if (s.phase === 'downloading') return 'Downloading update';
		if (s.phase === 'installing') return 'Installing update';
		return 'Update failed';
	});

	let titleClass = $derived(
		s.phase === 'error'
			? 'text-md font-semibold text-red-500'
			: 'text-md font-semibold text-emerald-700'
	);

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

{#if s.phase === 'up_to_date' || s.phase === 'available' || s.phase === 'downloading' || s.phase === 'installing' || s.phase === 'error'}
	<ModalShell
		{title}
		{titleClass}
		canClose={s.phase !== 'installing'}
		onClose={dismiss}
	>
		{#snippet badge()}
			{#if s.phase === 'available' || s.phase === 'downloading'}
				<span class="rounded bg-neutral-200 px-2 py-0.5 font-mono text-[10px] text-neutral-1000">
					v{s.update.version}
				</span>
			{/if}
		{/snippet}

		<div class="px-5 py-4">
			{#if s.phase === 'up_to_date'}
				<p class="text-xs text-neutral-1000">You're running the latest version.</p>
			{:else if s.phase === 'available'}
				<p class="mb-2 text-xs text-neutral-1000">
					A new version is ready to install. Your work will be saved before restarting.
				</p>
				{#if s.update.body}
					<pre class="max-h-60 overflow-auto rounded bg-neutral-200 p-3 font-mono text-[11px] leading-tight whitespace-pre-wrap break-words text-neutral-1100">{s.update.body}</pre>
				{/if}
			{:else if s.phase === 'downloading'}
				<div class="flex items-center gap-3 text-xs text-neutral-1000">
					<div class="h-2 flex-1 overflow-hidden rounded bg-neutral-300">
						<div class="h-full bg-emerald-500 transition-all" style="width: {progressPct()}%;"></div>
					</div>
					<span class="font-mono tabular-nums">{progressPct()}%</span>
				</div>
			{:else if s.phase === 'installing'}
				<p class="text-xs text-neutral-1000">Finalizing. The app will restart in a moment.</p>
			{:else if s.phase === 'error'}
				<p class="mb-2 text-xs text-neutral-1000">Couldn't check for updates.</p>
				<pre class="max-h-40 overflow-auto rounded bg-neutral-200 p-2 font-mono text-[11px] leading-tight whitespace-pre-wrap break-words text-neutral-1100">{s.message}</pre>
			{/if}
		</div>

		{#snippet footer()}
			{#if s.phase === 'up_to_date'}
				<button type="button" class="button-main primary rounded-lg" onclick={dismiss}>
					OK
				</button>
			{:else if s.phase === 'available'}
				<button type="button" class="button-main primary rounded-lg" onclick={onSkip}>
					Skip this version
				</button>
				<button type="button" class="button-main primary rounded-lg" onclick={dismiss}>
					Later
				</button>
				<button type="button" class="button-main green rounded-lg" onclick={() => installUpdate()}>
					Install &amp; restart
				</button>
			{:else if s.phase === 'error'}
				<button type="button" class="button-main primary rounded-lg" onclick={dismiss}>
					Dismiss
				</button>
				<CopyButton
					text={s.message}
					label="Copy error"
					class="button-main red gap-3 rounded-lg"
				/>
			{/if}
		{/snippet}
	</ModalShell>
{/if}
