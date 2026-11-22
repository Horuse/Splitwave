<script lang="ts">
	import { parseNotes, type AlertType, type Inline } from '#lib/changelog.js';

	let { body }: { body: string } = $props();

	const blocks = $derived(parseNotes(body));

	// Section dots reuse the landing page's node-category colours.
	function accent(content: Inline[]): string {
		const label = content.map((p) => ('text' in p ? p.text : '')).join('');
		if (/add|new|feature/i.test(label)) return 'bg-emerald-500';
		if (/fix/i.test(label)) return 'bg-sky-500';
		if (/remov|break|deprecat/i.test(label)) return 'bg-red-500';
		if (/chang|improv|updat|perf/i.test(label)) return 'bg-fuchsia-500';
		return 'bg-neutral-600';
	}

	const ALERTS: Record<AlertType, { label: string; box: string; title: string }> = {
		note: { label: 'Note', box: 'border-blue-500/60', title: 'text-blue-500' },
		tip: { label: 'Tip', box: 'border-emerald-500/60', title: 'text-emerald-500' },
		important: { label: 'Important', box: 'border-fuchsia-500/60', title: 'text-fuchsia-500' },
		warning: { label: 'Warning', box: 'border-amber-500/60', title: 'text-amber-500' },
		caution: { label: 'Caution', box: 'border-red-500/60', title: 'text-red-500' }
	};
</script>

{#snippet inline(content: Inline[])}
	{#each content as part, i (i)}
		{#if part.kind === 'text'}
			{part.text}
		{:else if part.kind === 'break'}
			<br />
		{:else if part.kind === 'code'}
			<code class="rounded bg-neutral-200 px-1 py-0.5 font-mono text-[0.8125rem] text-theme">
				{part.text}
			</code>
		{:else if part.kind === 'strong'}
			<strong class="font-semibold text-theme">{part.text}</strong>
		{:else if part.kind === 'em'}
			<em>{part.text}</em>
		{:else}
			<a
				href={part.href}
				target="_blank"
				rel="noopener noreferrer"
				class="text-theme underline decoration-neutral-500 underline-offset-2 transition-colors hover:decoration-theme"
			>
				{part.text}
			</a>
		{/if}
	{/each}
{/snippet}

<div class="flex flex-col gap-4">
	{#each blocks as block, i (i)}
		{#if block.kind === 'heading'}
			<h3 class="mt-4 flex items-center gap-2 text-sm font-semibold first:mt-0">
				<span class="size-2 rounded-full {accent(block.content)}"></span>
				{@render inline(block.content)}
			</h3>
		{:else if block.kind === 'list'}
			<ul class="flex list-disc flex-col gap-2 pl-5 marker:text-neutral-600">
				{#each block.items as item, j (j)}
					<li class="pl-1 text-sm leading-relaxed text-neutral-1000">
						{@render inline(item)}
					</li>
				{/each}
			</ul>
		{:else if block.kind === 'quote'}
			<blockquote
				class="rounded-xl border border-neutral-300 bg-neutral-100 px-4 py-3 text-sm leading-relaxed text-neutral-1000"
			>
				{@render inline(block.content)}
			</blockquote>
		{:else if block.kind === 'alert'}
			{@const a = ALERTS[block.type]}
			<div class="flex flex-col gap-1 rounded-xl border border-l-4 {a.box} px-4 py-3">
				<p class="text-xs font-semibold tracking-wide uppercase {a.title}">{a.label}</p>
				<p class="text-sm leading-relaxed text-neutral-1000">
					{@render inline(block.content)}
				</p>
			</div>
		{:else}
			<p class="text-sm leading-relaxed text-neutral-1000">
				{@render inline(block.content)}
			</p>
		{/if}
	{/each}
</div>
