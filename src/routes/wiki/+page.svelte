<script lang="ts">
	import Header from '$lib/components/layout/header.svelte';
	import HeaderNav from '$lib/components/layout/header_nav.svelte';
	import { t } from '$lib/modules/i18n';

	const ENTRIES = [
		{ key: 'multiselect', video: '/wiki/alt_connect.mp4' },
		{ key: 'copyPaste', video: '/wiki/ctrl_copy_paste.mp4' },
		{ key: 'boxSelect', video: '/wiki/shift_select.mp4' },
		{ key: 'deleteEdges', video: '/wiki/shift_delete_edges.mp4' }
	];

	const entries = $derived(
		ENTRIES.map((entry) => ({
			...entry,
			title: t(`wiki.${entry.key}.title`),
			body: t(`wiki.${entry.key}.body`)
		}))
	);
</script>

<Header>
	{#snippet left()}
		<HeaderNav />
	{/snippet}
</Header>

<div class="min-h-0 flex-1 overflow-y-auto p-8">
	<div class="flex max-w-2xl flex-col gap-8">
		{#each entries as entry (entry.key)}
			<section class="flex flex-col gap-3">
				<div>
					<h2 class="text-sm font-semibold text-theme">{entry.title}</h2>
					<p class="text-xs text-neutral-900">{entry.body}</p>
				</div>

				<video autoplay loop muted playsinline class="w-full rounded-xl border border-neutral-400 bg-neutral-100">
					<source src={entry.video} type="video/mp4" />
					<source src={entry.video.replace('.mp4', '.webm')} type="video/webm" />
				</video>
			</section>
		{/each}
	</div>
</div>
