<script lang="ts">
	import { tree, type WikiNode } from '@splitwave/wiki';

	/** Articles under a section, depth-first, for the index cards. */
	function pages(node: WikiNode): WikiNode[] {
		return [...(node.article ? [node] : []), ...node.children.flatMap(pages)];
	}
</script>

<div class="flex flex-col gap-10">
	<header class="flex flex-col gap-3">
		<h1 class="text-3xl font-bold tracking-tight">Splitwave Wiki</h1>
		<p class="max-w-2xl text-base leading-relaxed text-neutral-1000">
			Guides for building audio routing graphs in Splitwave: capturing microphones, system and app
			audio, shaping it with effects and plugins, and sending it to speakers, files, virtual devices
			or another machine.
		</p>
	</header>

	{#each tree as section (section.slug)}
		<section class="flex flex-col gap-3">
			<h2 class="text-xl font-semibold">
				{#if section.article}<a href="/wiki/{section.slug}" class="hover:underline">
						{section.title}
					</a>{:else}{section.title}{/if}
			</h2>
			{#if section.article?.description}
				<p class="text-sm text-neutral-1000">{section.article.description}</p>
			{/if}
			<ul class="grid gap-3 sm:grid-cols-2">
				{#each pages(section).filter((n) => n !== section) as node (node.slug)}
					<li>
						<a
							href="/wiki/{node.slug}"
							class="card flex h-full flex-col gap-1 transition-colors hover:border-neutral-500"
						>
							<span class="text-sm font-semibold text-theme">{node.title}</span>
							{#if node.article?.description}<span class="text-sm text-neutral-1000">
									{node.article.description}
								</span>{/if}
						</a>
					</li>
				{/each}
			</ul>
		</section>
	{/each}
</div>
