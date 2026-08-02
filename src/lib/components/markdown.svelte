<script lang="ts">
	import type { ClassValue } from 'svelte/elements';
	import { openUrl } from '@tauri-apps/plugin-opener';
	import { parseMarkdown, type Inline } from '$lib/utils/markdown';

	let { source, class: cls }: { source: string; class?: ClassValue } = $props();

	let blocks = $derived(parseMarkdown(source));
</script>

{#snippet inline(parts: Inline[])}
	{#each parts as part, i (i)}
		{#if part.kind === 'strong'}
			<strong class="font-semibold">{part.text}</strong>
		{:else if part.kind === 'em'}
			<em class="italic">{part.text}</em>
		{:else if part.kind === 'code'}
			<code class="rounded bg-neutral-300 px-1 font-mono text-[0.9em]">{part.text}</code>
		{:else if part.kind === 'link'}
			<!-- Opening in place would replace the app, so the link leaves the webview. -->
			<button
				type="button"
				class="break-all underline"
				onclick={() => openUrl(part.href).catch(() => {})}
			>
				{part.text}
			</button>
		{:else}
			{part.text}
		{/if}
	{/each}
{/snippet}

<div class={cls}>
	{#each blocks as block, i (i)}
		{#if block.kind === 'heading'}
			<p class="mt-3 text-[11px] font-semibold tracking-wide uppercase opacity-70 first:mt-0">
				{@render inline(block.content)}
			</p>
		{:else if block.kind === 'paragraph'}
			<p class="mt-1.5 first:mt-0">{@render inline(block.content)}</p>
		{:else if block.kind === 'list'}
			<svelte:element
				this={block.ordered ? 'ol' : 'ul'}
				class={['mt-1.5 pl-4 first:mt-0', block.ordered ? 'list-decimal' : 'list-disc']}
			>
				{#each block.items as item, j (j)}
					<li class="mt-0.5">{@render inline(item)}</li>
				{/each}
			</svelte:element>
		{:else}
			<pre
				class="mt-2 overflow-x-auto rounded-lg bg-neutral-300 p-2 font-mono text-[0.9em]">{block.text}</pre>
		{/if}
	{/each}
</div>
