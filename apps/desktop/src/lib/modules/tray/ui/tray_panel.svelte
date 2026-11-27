<script lang="ts">
	import { Button, ScrollArea } from '@splitwave/ui';
	import Play from '~icons/fluent/play-16-filled';
	import Stop from '~icons/fluent/stop-16-filled';
	import Open from '~icons/fluent/open-16-filled';
	import Power from '~icons/fluent/power-20-filled';
	import type { TrayPipeline } from '../types';

	let {
		pipelines,
		runningId,
		isRunning,
		busyId = null,
		error = null,
		onrun,
		onstop,
		onopen,
		onquit
	}: {
		pipelines: TrayPipeline[];
		runningId: string | null;
		isRunning: boolean;
		busyId?: string | null;
		error?: string | null;
		onrun: (pipelineId: string) => void;
		onstop: () => void;
		onopen: () => void;
		onquit: () => void;
	} = $props();

	const active = $derived(isRunning ? (pipelines.find((p) => p.id === runningId) ?? null) : null);

	function describe(p: TrayPipeline): string {
		if (p.stale) return 'Saved by a newer version';
		return p.outputs.length ? p.outputs.join(', ') : 'No output';
	}
</script>

<div class="flex h-full flex-col overflow-hidden rounded-xl border border-neutral-300 bg-background text-theme select-none">
	<header class="flex flex-col gap-0.5 border-b border-neutral-300 px-4 py-3">
		<span class="flex items-center gap-2 text-sm font-semibold">
			<span class={['size-2 shrink-0 rounded-full', active ? 'bg-brand' : 'bg-neutral-500']}></span>
			<span class="truncate">{active ? active.name : 'Nothing is running'}</span>
		</span>
		<span class="truncate pl-4 text-xs text-neutral-900">
			{active ? describe(active) : 'Pick a pipeline to start routing'}
		</span>
	</header>

	{#if error}
		<p role="alert" class="line-clamp-3 border-b border-neutral-300 bg-red-500/15 px-4 py-2 text-xs text-red-500">{error}</p>
	{/if}

	<ScrollArea class="min-h-0 flex-1" viewportClass="flex flex-col gap-1 p-2">
		{#each pipelines as p (p.id)}
			{@const running = isRunning && p.id === runningId}
			<Button
				variant="ghost"
				size="xl"
				class={['!h-auto w-full !justify-start !px-3 !py-2 text-left', running && 'bg-brand/10']}
				active={running}
				loading={busyId === p.id}
				loadingPosition="end"
				disabled={p.stale || (!!busyId && busyId !== p.id)}
				title={running ? `Stop ${p.name}` : `Run ${p.name}`}
				aria-pressed={running}
				onclick={() => (running ? onstop() : onrun(p.id))}>
				<span class="flex w-64 items-center gap-3">
					{#if running}
						<Stop class="size-4 shrink-0 text-brand" />
					{:else}
						<Play class="size-4 shrink-0 text-neutral-800" />
					{/if}
					<span class="flex min-w-0 flex-col">
						<span class="truncate text-sm font-medium text-theme">{p.name}</span>
						<span class="truncate text-xs font-normal text-neutral-900">{describe(p)}</span>
					</span>
				</span>
			</Button>
		{:else}
			<p class="px-3 py-6 text-center text-xs text-neutral-900">No pipelines yet. Create one in Splitwave.</p>
		{/each}
	</ScrollArea>

	<footer class="flex items-center justify-between gap-2 border-t border-neutral-300 p-2">
		<Button variant="ghost" onclick={onopen}>
			<Open class="size-4" />
			Open Splitwave
		</Button>
		<Button variant="ghost" onclick={onquit}>
			<Power class="size-4" />
			Quit
		</Button>
	</footer>
</div>
