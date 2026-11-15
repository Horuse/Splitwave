<script lang="ts">
	import { tick } from 'svelte';
	import { Card, Separator, Toc, type TocItem } from '@splitwave/ui';
	import { editUrl, getNode, neighbours } from '@splitwave/wiki';
	import { openUrl } from '@tauri-apps/plugin-opener';
	import IconEdit from '~icons/fluent/edit-16-filled';
	import IconPrev from '~icons/fluent/arrow-left-16-filled';
	import IconNext from '~icons/fluent/arrow-right-16-filled';
	import Header from '$lib/components/layout/header.svelte';

	let { data } = $props();
	let node = $derived(getNode(data.slug)!);
	let around = $derived(neighbours(data.slug));
	let crumbs = $derived(
		data.slug
			.split('/')
			.slice(0, -1)
			.map((_, i, parts) => parts.slice(0, i + 1).join('/'))
			.map((slug) => ({ slug, title: getNode(slug)?.title ?? slug }))
	);

	let scroller = $state<HTMLDivElement>();
	let body = $state<HTMLDivElement>();
	let headings = $state<TocItem[]>([]);
	let activeHeading = $state<string | null>(null);

	const slugify = (text: string) =>
		text
			.toLowerCase()
			.replace(/[^a-z0-9]+/g, '-')
			.replace(/^-|-$/g, '');

	// mdsvex output has no heading ids, so they are assigned here and read back for the table of contents.
	$effect(() => {
		void data.slug;
		tick().then(() => {
			const found = [...(body?.querySelectorAll<HTMLHeadingElement>('h2, h3') ?? [])];
			headings = found.map((h) => {
				h.id ||= slugify(h.textContent ?? '');
				return { id: h.id, label: h.textContent ?? '', level: Number(h.tagName[1]) };
			});
			scroller?.scrollTo({ top: 0 });
			updateActive();
		});
	});

	function updateActive() {
		if (!scroller || !headings.length) {
			activeHeading = null;
			return;
		}
		const limit = scroller.getBoundingClientRect().top + 96;
		let current = headings[0].id;
		for (const h of headings) {
			const el = document.getElementById(h.id);
			if (el && el.getBoundingClientRect().top <= limit) current = h.id;
		}
		activeHeading = current;
	}
</script>

<Header />

<div bind:this={scroller} class="min-h-0 flex-1 overflow-y-auto" onscroll={updateActive}>
	<div class="mx-auto flex w-full max-w-5xl gap-10 p-8">
		<article
			class="flex min-w-0 flex-1 flex-col text-sm leading-relaxed text-neutral-1100 [&_h2]:mt-8 [&_h2]:scroll-mt-6 [&_h2]:text-lg [&_h2]:font-semibold [&_h2]:text-theme [&_h3]:mt-6 [&_h3]:scroll-mt-6 [&_h3]:font-semibold [&_h3]:text-theme [&_p]:mt-3 [&_strong]:font-semibold [&_strong]:text-theme [&_ul]:mt-3 [&_ul]:list-disc [&_ul]:pl-5">
			<nav class="flex flex-wrap items-center gap-1 text-xs text-neutral-900">
				<a href="/wiki" class="transition-colors duration-150 hover:text-theme">Wiki</a>
				{#each crumbs as c (c.slug)}
					<span>/</span>
					<a href="/wiki/{c.slug}" class="transition-colors duration-150 hover:text-theme">{c.title}</a>
				{/each}
			</nav>
			<h1 class="mt-1 text-2xl font-semibold text-theme">{node.title}</h1>

			<div bind:this={body}>
				{#if node.article}
					<node.article.component />
				{/if}
			</div>

			{#if node.children.length}
				<div class="mt-6 grid grid-cols-1 gap-3 sm:grid-cols-2">
					{#each node.children as child (child.slug)}
						<a href="/wiki/{child.slug}" class="block">
							<Card class="flex h-full flex-col gap-1 p-5 hover:border-neutral-500">
								<span class="text-sm font-semibold text-theme">{child.title}</span>
								{#if child.article?.description}<span class="text-xs text-neutral-900">{child.article.description}</span>{/if}
							</Card>
						</a>
					{/each}
				</div>
			{/if}

			<Separator class="mt-10" />

			{#if node.article}
				{@const article = node.article}
				<button
					type="button"
					class="mt-4 flex items-center gap-1.5 self-start text-xs text-neutral-900 transition-colors duration-150 hover:text-theme"
					onclick={() => openUrl(editUrl(article)).catch(() => {})}>
					<IconEdit class="size-3.5" />
					Edit this page on GitHub
				</button>
			{/if}

			<div class="mt-4 grid grid-cols-2 gap-3">
				{#if around.prev}
					<a href="/wiki/{around.prev.slug}" class="block">
						<Card class="flex h-full flex-col gap-1 p-4 hover:border-neutral-500">
							<span class="flex items-center gap-1 text-xs text-neutral-900"><IconPrev class="size-3" /> Previous</span>
							<span class="text-sm font-semibold text-theme">{around.prev.title}</span>
						</Card>
					</a>
				{:else}
					<span></span>
				{/if}
				{#if around.next}
					<a href="/wiki/{around.next.slug}" class="block">
						<Card class="flex h-full flex-col items-end gap-1 p-4 text-right hover:border-neutral-500">
							<span class="flex items-center gap-1 text-xs text-neutral-900">Next <IconNext class="size-3" /></span>
							<span class="text-sm font-semibold text-theme">{around.next.title}</span>
						</Card>
					</a>
				{/if}
			</div>
		</article>

		{#if headings.length}
			<aside class="sticky top-0 hidden w-48 shrink-0 self-start lg:block">
				<Toc items={headings} activeId={activeHeading} />
			</aside>
		{/if}
	</div>
</div>
