<script lang="ts">
	import { ModalShell } from '$lib/modules/overlay/ui';
	import CopyButton from '$lib/components/copy_button.svelte';
	import { logStore } from '../stores.svelte';
	import type { LogEntry } from '../types';

	const LEVELS = ['ALL', 'ERROR', 'WARN', 'INFO', 'DEBUG', 'TRACE'];

	let query = $state('');
	let level = $state('ALL');

	let filtered = $derived(
		logStore.entries.filter((e) => {
			if (level !== 'ALL' && e.level.toUpperCase() !== level) return false;
			const q = query.trim().toLowerCase();
			if (!q) return true;
			return `${e.target} ${e.message}`.toLowerCase().includes(q);
		})
	);

	function time(at: number): string {
		return new Date(at).toLocaleTimeString(undefined, { hour12: false });
	}

	function levelClass(l: string): string {
		switch (l.toUpperCase()) {
			case 'ERROR':
				return 'text-red-500';
			case 'WARN':
				return 'text-yellow-500';
			case 'INFO':
				return 'text-green-500';
			case 'DEBUG':
				return 'text-sky-500';
			case 'TRACE':
				return 'text-violet-400';
			default:
				return 'text-neutral-900';
		}
	}

	/** `key=value` pairs come from `tracing` fields; the key is dimmed and
	 * italicised the way the terminal formatter renders them. */
	function fields(message: string): { key?: string; text: string }[] {
		const out: { key?: string; text: string }[] = [];
		let last = 0;
		for (const m of message.matchAll(/(?<=^|\s)([A-Za-z_][\w.]*)=/g)) {
			if (m.index > last) out.push({ text: message.slice(last, m.index) });
			out.push({ key: m[1], text: '=' });
			last = m.index + m[0].length;
		}
		if (last < message.length) out.push({ text: message.slice(last) });
		return out;
	}

	function asText(list: LogEntry[]): string {
		return list
			.map((e) => `${time(e.at)} ${e.level.padEnd(5)} [${e.target}] ${e.message}`)
			.join('\n');
	}

	// The backend buffer is polled -- tracing events carry no Tauri event of their own.
	$effect(() => {
		logStore.refresh().catch(() => {});
		const timer = setInterval(() => logStore.refresh().catch(() => {}), 1000);
		return () => clearInterval(timer);
	});
</script>

<ModalShell title="Logs" size="xl" onClose={() => (logStore.open = false)}>
	{#snippet badge()}
		<span class="rounded-md bg-neutral-200 px-2 py-0.5 font-mono text-[10px] text-neutral-1000">
			{filtered.length}
		</span>
	{/snippet}

	<div class="flex flex-col gap-3 px-5 py-4">
		<div class="flex items-center gap-2">
			<input
				bind:value={query}
				type="text"
				placeholder="Filter…"
				spellcheck="false"
				class="flex-1 rounded-md border border-neutral-400 bg-neutral-100 px-2 py-1 text-sm outline-none"
			/>
			<div class="flex items-center gap-1">
				{#each LEVELS as l (l)}
					<button
						type="button"
						class={[
							'rounded-md px-2 py-1 font-mono text-[10px]',
							level === l ? 'bg-neutral-300 text-neutral-1100' : 'text-neutral-900 hover:bg-neutral-200'
						]}
						onclick={() => (level = l)}
					>
						{l}
					</button>
				{/each}
			</div>
		</div>

		<div
			class="h-[26rem] overflow-auto rounded-lg border border-neutral-300 bg-neutral-200 p-2 font-mono text-[11px] leading-relaxed"
		>
			{#each filtered as e (e.origin + e.at + e.message)}
				<div class="flex gap-2 whitespace-pre-wrap break-words">
					<span class="shrink-0 tabular-nums text-neutral-800">{time(e.at)}</span>
					<span class="w-10 shrink-0 font-semibold {levelClass(e.level)}">
						{e.level.toUpperCase()}
					</span>
					<span class="shrink-0 text-neutral-900">{e.target}:</span>
					<span class="text-neutral-1100">
						{#each fields(e.message) as part, i (i)}
							{#if part.key}
								<span class="text-neutral-900 italic">{part.key}</span>{part.text}
							{:else}
								{part.text}
							{/if}
						{/each}
					</span>
				</div>
			{:else}
				<p class="p-2 text-neutral-900 italic">No log lines.</p>
			{/each}
		</div>
	</div>

	{#snippet footer()}
		<button
			type="button"
			class="button-main primary rounded-lg"
			onclick={() => logStore.clear()}
		>
			Clear
		</button>
		<CopyButton text={() => asText(filtered)} label="Copy" />
		<button
			type="button"
			class="button-main green rounded-lg"
			onclick={() => (logStore.open = false)}
		>
			Close
		</button>
	{/snippet}
</ModalShell>
