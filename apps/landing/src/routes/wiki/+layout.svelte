<script lang="ts">
	import { page } from '$app/state';
	import { tree, type WikiNode } from '@splitwave/wiki';

	let { children } = $props();

	let current = $derived(page.params.slug ?? '');
</script>

{#snippet nav(list: WikiNode[], depth: number)}
	<ul class={['flex flex-col', depth > 0 && 'mt-0.5 ml-3 border-l border-neutral-200 pl-2']}>
		{#each list as node (node.slug)}
			<li>
				{#if node.article}
					<a
						href="/wiki/{node.slug}"
						aria-current={node.slug === current ? 'page' : undefined}
						class={[
							'block rounded-md px-2 py-1 text-sm transition-colors',
							node.slug === current
								? 'bg-neutral-200 font-medium text-theme'
								: 'text-neutral-1000 hover:text-theme'
						]}
					>
						{node.title}
					</a>
				{:else}
					<span
						class="block px-2 pt-3 pb-1 text-xs font-semibold tracking-wide text-neutral-900 uppercase"
					>
						{node.title}
					</span>
				{/if}
				{#if node.children.length}
					{@render nav(node.children, depth + 1)}
				{/if}
			</li>
		{/each}
	</ul>
{/snippet}

<div class="mx-auto grid max-w-5xl gap-10 px-6 py-12 md:grid-cols-[13rem_1fr]">
	<aside class="md:sticky md:top-24 md:max-h-[calc(100vh-7rem)] md:self-start md:overflow-y-auto">
		<a
			href="/wiki"
			class={[
				'mb-2 block px-2 text-sm font-semibold',
				current === '' ? 'text-theme' : 'text-neutral-1100 hover:text-theme'
			]}
		>
			Wiki
		</a>
		<nav aria-label="Wiki">{@render nav(tree, 0)}</nav>
	</aside>
	<div class="min-w-0">{@render children()}</div>
</div>
