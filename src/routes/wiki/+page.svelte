<script lang="ts">
	import { fly } from 'svelte/transition';
	import { Card } from '@splitwave/ui';
	import { tree } from '@splitwave/wiki';
	import Header from '$lib/components/layout/header.svelte';
</script>

<Header />

<div class="min-h-0 flex-1 overflow-y-auto">
	<div class="mx-auto flex w-full max-w-3xl flex-col gap-5 p-8">
		<div class="flex flex-col gap-1">
			<h1 class="text-2xl font-semibold">Wiki</h1>
			<p class="text-sm text-neutral-900">Tips and guides for building pipelines.</p>
		</div>

		<div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
			{#each tree as a, i (a.slug)}
				<a href="/wiki/{a.slug}" class="block" in:fly={{ y: 8, duration: 200, delay: i * 40 }}>
					<Card class="flex h-full flex-col gap-1 p-5 hover:border-neutral-500">
						<h2 class="text-sm font-semibold">{a.title}</h2>
						<p class="text-xs text-neutral-900">{a.article?.description ?? ''}</p>
					</Card>
				</a>
			{/each}
		</div>
	</div>
</div>
