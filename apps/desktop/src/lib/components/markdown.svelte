<script lang="ts">
	import type { ClassValue } from 'svelte/elements';
	import { openUrl } from '@tauri-apps/plugin-opener';
	import Info from '~icons/fluent/info-16-regular';
	import Lightbulb from '~icons/fluent/lightbulb-16-regular';
	import Megaphone from '~icons/fluent/megaphone-16-regular';
	import Warning from '~icons/fluent/warning-16-regular';
	import ErrorCircle from '~icons/fluent/error-circle-16-regular';
	import Checkmark from '~icons/fluent/checkmark-12-filled';
	import Mermaid from './mermaid.svelte';
	import { parseMarkdown, type Block, type CalloutKind, type Inline } from '$lib/utils/markdown';

	interface Props {
		source: string;
		/** `compact` for release notes in cards and banners; `article` for a full announcement page. */
		variant?: 'compact' | 'article';
		class?: ClassValue;
	}

	let { source, variant = 'compact', class: cls }: Props = $props();

	let blocks = $derived(parseMarkdown(source));
	let article = $derived(variant === 'article');

	const HEADING_ARTICLE = [
		'',
		'text-2xl font-semibold',
		'text-xl font-semibold',
		'text-lg font-semibold',
		'text-base font-semibold',
		'text-sm font-semibold',
		'text-sm font-semibold'
	];

	const CALLOUT: Record<CalloutKind, { label: string; icon: typeof Info; class: string }> = {
		note: { label: 'Note', icon: Info, class: 'border-sky-500/40 bg-sky-500/10 text-sky-500' },
		tip: { label: 'Tip', icon: Lightbulb, class: 'border-emerald-500/40 bg-emerald-500/10 text-emerald-500' },
		important: { label: 'Important', icon: Megaphone, class: 'border-violet-500/40 bg-violet-500/10 text-violet-500' },
		warning: { label: 'Warning', icon: Warning, class: 'border-amber-500/40 bg-amber-500/10 text-amber-500' },
		caution: { label: 'Caution', icon: ErrorCircle, class: 'border-red-500/40 bg-red-500/10 text-red-500' }
	};

	function open(href: string) {
		// Opening in place would replace the app, so links leave the webview.
		openUrl(href).catch(() => {});
	}
</script>

{#snippet inline(parts: Inline[])}
	{#each parts as part, i (i)}
		{#if part.kind === 'text'}
			{part.text}
		{:else if part.kind === 'strong'}
			<strong class="font-semibold">{@render inline(part.children)}</strong>
		{:else if part.kind === 'em'}
			<em class="italic">{@render inline(part.children)}</em>
		{:else if part.kind === 'del'}
			<del class="opacity-70">{@render inline(part.children)}</del>
		{:else if part.kind === 'code'}
			<code class="rounded-md bg-violet-500/12 px-[0.35em] py-[0.15em] font-mono text-[0.85em] text-violet-500">{part.text}</code>
		{:else if part.kind === 'link'}
			<a
				href={part.href}
				class="cursor-pointer break-words text-violet-500 underline-offset-2 hover:text-violet-600 hover:underline"
				onclick={(e) => {
					e.preventDefault();
					open(part.href);
				}}>
				{@render inline(part.children)}
			</a>
		{:else if part.kind === 'image'}
			<img src={part.src} alt={part.alt} title={part.title} loading="lazy" class="inline-block max-w-full rounded-lg align-middle" />
		{:else if part.kind === 'br'}
			<br />
		{/if}
	{/each}
{/snippet}

{#snippet render(list: Block[])}
	{#each list as block, i (i)}
		{#if block.kind === 'heading'}
			{#if article}
				<svelte:element
					this={`h${block.level}`}
					class={['mt-6 mb-3 leading-tight text-pretty text-neutral-1200 first:mt-0', HEADING_ARTICLE[block.level]]}>
					{@render inline(block.content)}
				</svelte:element>
			{:else}
				<p class="mt-4 mb-1 text-[13px] leading-tight font-semibold text-neutral-1200 first:mt-0">{@render inline(block.content)}</p>
			{/if}
		{:else if block.kind === 'paragraph'}
			{#if block.content.length === 1 && block.content[0].kind === 'image'}
				{@const img = block.content[0]}
				<figure class="my-4 first:mt-0 last:mb-0">
					<img src={img.src} alt={img.alt} loading="lazy" class="w-full rounded-lg border border-theme/5" />
					{#if img.title || img.alt}
						<figcaption class="mt-1.5 text-center text-xs text-neutral-700">{img.title || img.alt}</figcaption>
					{/if}
				</figure>
			{:else}
				<p class={['my-2 break-words first:mt-0 last:mb-0', article ? 'leading-6' : 'leading-relaxed']}>{@render inline(block.content)}</p>
			{/if}
		{:else if block.kind === 'hr'}
			<hr class="my-6 border-t-2 border-theme/10" />
		{:else if block.kind === 'quote'}
			<blockquote class="my-4 rounded-sm border-l-4 border-neutral-500 bg-neutral-200/40 py-1 pr-3 pl-4 text-neutral-1000 first:mt-0 last:mb-0">
				{@render render(block.blocks)}
			</blockquote>
		{:else if block.kind === 'callout'}
			{@const c = CALLOUT[block.callout]}
			<div class={['my-4 rounded-lg border px-3 py-2 first:mt-0 last:mb-0', c.class]}>
				<p class="mb-1 flex items-center gap-1.5 text-xs font-semibold"><c.icon class="size-3.5" />{c.label}</p>
				<div class="text-neutral-1100">{@render render(block.blocks)}</div>
			</div>
		{:else if block.kind === 'list'}
			<svelte:element
				this={block.ordered ? 'ol' : 'ul'}
				start={block.ordered && block.start !== 1 ? block.start : undefined}
				class={['my-3 ml-1.5 pl-4 marker:text-neutral-700 first:mt-0 last:mb-0', block.ordered ? 'list-decimal' : 'list-disc']}>
				{#each block.items as item, j (j)}
					<li class={['my-1 pl-0.5', item.checked !== null && '-ml-4 list-none']}>
						{#if item.checked !== null}
							<span class="flex items-start gap-2">
								<span
									class={[
										'mt-0.5 flex size-4 shrink-0 items-center justify-center rounded-md border',
										item.checked ? 'border-emerald-600 bg-emerald-600 text-white' : 'border-neutral-400 bg-neutral-200'
									]}>
									{#if item.checked}<Checkmark class="size-3" />{/if}
								</span>
								<span class={['min-w-0 flex-1', item.checked && 'opacity-70']}>{@render render(item.blocks)}</span>
							</span>
						{:else}
							{@render render(item.blocks)}
						{/if}
					</li>
				{/each}
			</svelte:element>
		{:else if block.kind === 'code'}
			<div class="relative my-4 first:mt-0 last:mb-0">
				{#if block.lang}
					<span class="absolute top-1.5 right-2 font-mono text-[10px] text-neutral-600">{block.lang}</span>
				{/if}
				<pre class="overflow-x-auto rounded-lg bg-neutral-100 px-4 py-3 font-mono text-[0.8rem] leading-relaxed text-neutral-1100">{block.text}</pre>
			</div>
		{:else if block.kind === 'mermaid'}
			<div class="my-4 first:mt-0 last:mb-0"><Mermaid source={block.text} /></div>
		{:else if block.kind === 'table'}
			<div class="my-4 overflow-x-auto first:mt-0 last:mb-0">
				<table class="w-full border-collapse text-[0.95em]">
					<thead>
						<tr>
							{#each block.head as cell, c (c)}
								<th class="border border-theme/10 bg-theme/5 px-3 py-2 align-top font-semibold" style:text-align={block.align[c] ?? 'left'}>
									{@render inline(cell)}
								</th>
							{/each}
						</tr>
					</thead>
					<tbody>
						{#each block.rows as row, r (r)}
							<tr>
								{#each row as cell, c (c)}
									<td class="border border-theme/10 px-3 py-2 align-top" style:text-align={block.align[c] ?? 'left'}>
										{@render inline(cell)}
									</td>
								{/each}
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
		{/if}
	{/each}
{/snippet}

<div class={cls}>{@render render(blocks)}</div>
