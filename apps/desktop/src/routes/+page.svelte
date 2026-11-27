<script lang="ts">
	import Header from '$lib/components/layout/header.svelte';
	import PipelinePreview from '$lib/modules/pipeline/ui/pipeline_preview.svelte';
	import { slide } from 'svelte/transition';
	import { flip } from 'svelte/animate';
	import { Button, Card, Menu, MenuItem, MenuSeparator, Popover, smoothRadius } from '@splitwave/ui';
	import { goto } from '$app/navigation';
	import { methods as pipelineMethods } from '$lib/modules/pipeline/methods';
	import { pipelineStore } from '$lib/modules/pipeline/stores.svelte';
	import { modalManager } from '$lib/modules/overlay/modal';
	import { ConfirmModal } from '$lib/modules/overlay/ui';
	import { relativeTime } from '$lib/utils/time';
	import { isFromFuture } from '$lib/modules/pipeline/migrations';
	import { TEMPLATES, instantiate } from '$lib/modules/template';
	import { TemplateModal } from '$lib/modules/template/ui';

	async function createPipeline() {
		const templateId = await modalManager.open<string>('New pipeline', TemplateModal, {
			size: 'xl',
			description: 'Start from a wired-up layout, or an empty canvas.'
		});
		if (!templateId) return;

		const template = TEMPLATES.find((t) => t.id === templateId);
		if (!template) return;

		const name = `Pipeline ${pipelineStore.pipelines.length + 1}`;
		const p = instantiate(template, name);
		await pipelineStore.save(p);
		await goto(`/pipelines/${p.id}`);
	}

	let menuFor = $state<string | null>(null);

	async function remove(id: string, name: string) {
		const ok = await modalManager.open<boolean>(`Delete "${name}"?`, ConfirmModal, {
			message: 'This pipeline and its snapshot history will be permanently removed.',
			confirmLabel: 'Delete',
			danger: true
		});
		if (!ok) return;
		if (audioStore.isRunningPipeline(id)) await audioStore.deactivatePipeline();
		await pipelineStore.remove(id);
	}

	// Fresh node ids: the engine keys effect state by them, so a copy must not share them.
	async function duplicate(id: string) {
		const src = await pipelineMethods.get(id);
		if (!src || isFromFuture(src)) return;
		const ids = new Map(src.nodes.map((n) => [n.id, createId()]));
		const now = Date.now();
		await pipelineStore.save({
			...src,
			id: createId(),
			name: `${src.name} copy`,
			nodes: src.nodes.map((n) => ({ ...n, id: ids.get(n.id)! })),
			edges: src.edges.map((e) => {
				const source = ids.get(e.source)!;
				const target = ids.get(e.target)!;
				return { ...e, id: `${source}-${e.sourceHandle}-${target}-${e.targetHandle}`, source, target };
			}),
			createdAt: now,
			updatedAt: now
		});
	}

	import { audioStore } from '$lib/modules/audio/stores.svelte';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import { RunButton, RunningTimer, DriverUpdateBanner } from '$lib/modules/audio/ui';
	import Add from '~icons/fluent/add-24-filled';
	import Delete from '~icons/fluent/delete-16-filled';
	import Copy from '~icons/fluent/copy-16-filled';
	import { createId } from '@paralleldrive/cuid2';
	import More from '~icons/fluent/more-horizontal-24-filled';
	import SoundWave from '~icons/fluent/sound-wave-circle-16-filled';
	import { platform } from '@tauri-apps/plugin-os';

	const isWindows = platform() === 'windows';

	let busy = $state<string | null>(null);

	async function toggle(id: string, event: Event) {
		event.stopPropagation();
		event.preventDefault();
		if (busy) return;
		busy = id;
		try {
			if (audioStore.isRunningPipeline(id)) {
				await audioStore.deactivatePipeline();
			} else {
				const p = await pipelineMethods.get(id);
				if (!p || isFromFuture(p)) return;
				await audioStore.activatePipeline(id, { nodes: p.nodes, edges: p.edges });
			}
		} catch (e) {
			audioStore.reportError(e);
		} finally {
			busy = null;
		}
	}
</script>

<Header />

<div class="min-h-0 flex-1 overflow-y-auto">
	<div class="flex w-full flex-col gap-5 p-8">
		{#if !isWindows}
			<DriverUpdateBanner />
		{/if}

		<div class="flex items-end justify-between gap-4">
			<div class="flex flex-col gap-1">
				<h1 class="text-2xl font-semibold">Pipelines</h1>
				<p class="text-sm text-neutral-900">Route audio between devices and apps through a graph of nodes.</p>
			</div>
			<Button tone="brand" onclick={createPipeline}>
				<Add class="size-4" />
				New pipeline
			</Button>
		</div>

		{#if pipelineStore.pipelines.length === 0}
			<Card dashed class="flex flex-col items-center gap-3 py-16">
				<SoundWave class="size-10 text-neutral-600" />
				<p class="text-sm text-neutral-900">No pipelines yet. Create one to get started.</p>
				<Button tone="brand" onclick={createPipeline}>
					<Add class="size-4" />
					New pipeline
				</Button>
			</Card>
		{:else}
			<div class="flex flex-col gap-3">
				{#each pipelineStore.pipelines as p (p.id)}
					{@const stale = isFromFuture(p)}
					{@const isRunning = audioStore.isRunningPipeline(p.id)}
					<div animate:flip={{ duration: 200 }} transition:slide={{ duration: 200 }}>
						<Card nested class={['flex items-center gap-4 p-3', isRunning ? 'border-brand/30 bg-brand/5' : !stale && 'hover:border-neutral-500']}>
							<svelte:element
								this={stale ? 'div' : 'a'}
								href={stale ? undefined : `/pipelines/${p.id}`}
								class="h-28 w-52 shrink-0 overflow-hidden bg-background p-2"
								{@attach smoothRadius({ variable: '--smooth-r' })}
								style="border-radius: var(--smooth-r, 12px)">
								<PipelinePreview nodes={p.nodes} edges={p.edges} />
							</svelte:element>
							<svelte:element
								this={stale ? 'div' : 'a'}
								href={stale ? undefined : `/pipelines/${p.id}`}
								class={['flex min-w-0 flex-1 flex-col gap-0.5', stale && 'opacity-60']}>
								<span class="flex items-center gap-2">
									<span class="truncate font-medium">{p.name}</span>
									{#if stale}
										<span class="rounded-full border border-amber-600/50 bg-amber-500/20 px-2 py-0.5 text-[10px] text-amber-600">
											newer version
										</span>
									{/if}
								</span>
								<span class="text-xs text-neutral-900">
									{#if stale}
										Saved by a newer version - update Splitwave to open it.
									{:else if isRunning}
										<span class="text-brand">Running <RunningTimer /></span>
									{:else}
										{p.nodes.length} nodes, updated {relativeTime(p.updatedAt)}
									{/if}
								</span>
							</svelte:element>
							<div class="flex shrink-0 items-center gap-2">
								<RunButton
									size="lg"
									state={isRunning ? 'stop' : audioStore.isRunning ? 'switch' : 'run'}
									loading={busy === p.id}
									disabled={stale || (!!busy && busy !== p.id)}
									onclick={(e) => toggle(p.id, e)} />
								<Popover placement="bottom-end" bind:open={() => menuFor === p.id, (v) => (menuFor = v ? p.id : null)}>
									{#snippet trigger()}
										<Button icon size="lg" aria-label="More" title="More">
											<More class="size-5" />
										</Button>
									{/snippet}
									<Menu>
										<MenuItem
											label="Duplicate"
											icon={Copy}
											disabled={stale}
											onclick={() => {
												menuFor = null;
												duplicate(p.id);
											}} />
										<MenuSeparator />
										<MenuItem
											label="Delete"
											icon={Delete}
											danger
											onclick={() => {
												menuFor = null;
												remove(p.id, p.name);
											}} />
									</Menu>
								</Popover>
							</div>
						</Card>
					</div>
				{/each}
			</div>
		{/if}
	</div>
</div>
