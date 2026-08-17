<script lang="ts">
	import { page } from '$app/state';
	import Header from '$lib/components/layout/header.svelte';


	const ENTRIES = [
		{
			title: 'Multi-select channels with Option',
			video: '/wiki/alt_connect.mp4',
			body: 'Hold Option (Alt on Windows/Linux) while picking channels to select several at once, then drag them together as one connection.'
		},
		{
			title: 'Select, copy and paste nodes',
			video: '/wiki/ctrl_copy_paste.mp4',
			body: 'Press Ctrl+A (Cmd+A) to select every node on the canvas. Copy with Ctrl+C and paste with Ctrl+V (Cmd on macOS) to duplicate the selection.'
		},
		{
			title: 'Box select with Shift',
			video: '/wiki/shift_select.mp4',
			body: 'Hold Shift to switch the canvas into box-select mode, then drag a rectangle over the nodes you want to select.'
		},
		{
			title: 'Delete multiple connections at once',
			video: '/wiki/shift_delete_edges.mp4',
			body: 'Hold Shift and click each connection you want to remove, then press Delete to drop them all at once.'
		}
	];
</script>

<Header>
	{#snippet left()}
		<div class="flex items-center gap-2">
			<a class:active={page.route.id === '/'} href="/" class="button-header px-4 text-sm">Pipelines</a>
			<a class:active={page.route.id === '/virtual-devices'} href="/virtual-devices" class="button-header px-4 text-sm">Virtual devices</a>
			<a class:active={page.route.id === '/wiki'} href="/wiki" class="button-header px-4 text-sm">Wiki</a>
			<a class:active={page.route.id === '/settings'} href="/settings" class="button-header px-4 text-sm">Settings</a>
		</div>
	{/snippet}
</Header>

<div class="h-[calc(100vh-40px)] overflow-y-auto p-8">
	<div class="flex max-w-2xl flex-col gap-8">
		{#each ENTRIES as entry (entry.title)}
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
