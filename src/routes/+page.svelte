<script lang="ts">
	import { goto } from '$app/navigation';
	import { methods as pipelineMethods } from '$lib/modules/pipeline/methods';
	import { pipelineStore } from '$lib/modules/pipeline/stores.svelte';
	import { modalManager } from '$lib/modules/overlay/modal';
	import { ConfirmModal } from '$lib/modules/overlay/ui';
	import { relativeTime } from '$lib/utils/time';
	import { isFromFuture } from '$lib/modules/pipeline/migrations';
	import { TEMPLATES, instantiate } from '$lib/modules/template';
	import { TemplateModal } from '$lib/modules/template/ui';
	import { t } from '$lib/modules/i18n';

	async function createPipeline() {
		const templateId = await modalManager.open<string>(t('template.newTitle'), TemplateModal, {
			size: 'xl',
			description: t('template.newDescription')
		});
		if (!templateId) return;

		const template = TEMPLATES.find((tpl) => tpl.id === templateId);
		if (!template) return;

		const name = t('pipelines.defaultName', { n: pipelineStore.pipelines.length + 1 });
		const p = instantiate(template, name);
		await pipelineStore.save(p);
		await goto(`/pipelines/${p.id}`);
	}

	async function remove(id: string, name: string, event: Event) {
		event.stopPropagation();
		event.preventDefault();
		const ok = await modalManager.open<boolean>(t('pipelines.deleteTitle', { name }), ConfirmModal, {
			message: t('pipelines.deleteMessage'),
			confirmLabel: t('common.delete'),
			danger: true
		});
		if (ok) await pipelineStore.remove(id);
	}

	import Header from '$lib/components/layout/header.svelte';
	import HeaderNav from '$lib/components/layout/header_nav.svelte';
	import { audioStore } from '$lib/modules/audio/stores.svelte';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import { RunningTimer, DriverUpdateBanner } from '$lib/modules/audio/ui';
	import { Add, Delete, Play, SoundWave, Stop } from '$lib/components/icons';
	import { platform } from '@tauri-apps/plugin-os';

	const isWindows = platform() === 'windows';

	let busy = $state<string | null>(null);

	async function toggle(id: string, event: Event) {
		event.stopPropagation();
		event.preventDefault();
		if (busy) return;
		busy = id;
		try {
			if (audioStore.isRunning) {
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

<Header>
	{#snippet left()}
		<HeaderNav />
	{/snippet}
</Header>

<div class="flex min-h-0 flex-1 flex-col gap-8 overflow-y-auto p-8">
	{#if !isWindows}
		<DriverUpdateBanner />
	{/if}

	<div class="flex items-center justify-between">
		<h1 class="text-2xl font-semibold">{t('pipelines.title')}</h1>

		<button class="button-main primary gap-1.5 p-6 py-2" onclick={createPipeline}>
			<Add class="size-4" />
			{t('pipelines.new')}
		</button>
	</div>

	{#if pipelineStore.pipelines.length === 0}
		<div class="flex flex-col items-center gap-3 rounded-2xl border border-dashed border-neutral-400 py-16">
			<SoundWave class="size-10 text-neutral-600" />
			<p class="text-sm text-neutral-900">{t('pipelines.empty')}</p>
			<button class="button-main primary gap-1.5 py-1.5" onclick={createPipeline}>
				<Add class="size-4" />
				{t('pipelines.new')}
			</button>
		</div>
	{:else}
		<ul class="flex flex-col gap-4">
			{#each pipelineStore.pipelines as p (p.id)}
				{@const stale = isFromFuture(p)}
				<li class={['flex items-center rounded-2xl transition-colors', stale ? 'bg-neutral-100' : 'bg-neutral-200 hover:bg-neutral-300']}>
					<svelte:element this={stale ? 'div' : 'a'} href={stale ? undefined : `/pipelines/${p.id}`} class={['flex-1 p-4', stale && 'opacity-60']}>
						<div class="flex items-center gap-2">
							{#if audioStore.runningPipelineId === p.id}
								<span class="size-2 rounded-full bg-green-500"></span>
							{/if}
							<span class="font-medium">{p.name}</span>
							{#if stale}
								<span class="rounded border border-amber-600/50 bg-amber-500/20 px-1.5 py-0.5 font-mono text-[9px] text-amber-600">
									{t('pipelines.newerVersion')}
								</span>
							{/if}
							{#if audioStore.runningPipelineId === p.id}
								<RunningTimer />
							{/if}
						</div>
						<div class="text-xs text-neutral-900">
							{#if stale}
								{t('pipelines.newerVersionHint')}
							{:else}
								{t('pipelines.summary', { nodes: p.nodes.length, time: relativeTime(p.updatedAt) })}
							{/if}
						</div>
					</svelte:element>
					<div class="mx-4 flex h-full items-center gap-2 py-3.5">
						<button
							class={['btn-green py-2', !audioStore.isRunning && 'green', audioStore.isRunning && audioStore.runningPipelineId === p.id && 'red']}
							disabled={stale || !!busy || (audioStore.isRunning && audioStore.runningPipelineId !== p.id)}
							onclick={(e) => toggle(p.id, e)}>
							{#if busy === p.id}
								…
							{:else if audioStore.isRunning && audioStore.runningPipelineId === p.id}
								<Stop class="size-3.5" />
								{t('common.stop')}
							{:else}
								<Play class="size-3.5" />
								{t('common.activate')}
							{/if}
						</button>
						<button class="btn-alert py-2" onclick={(e) => remove(p.id, p.name, e)} aria-label={t('pipelines.deleteAria')}>
							<Delete class="size-3.5" />
							{t('common.delete')}
						</button>
					</div>
				</li>
			{/each}
		</ul>
	{/if}
</div>
