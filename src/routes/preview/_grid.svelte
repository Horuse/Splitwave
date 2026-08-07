<script lang="ts">
	import { setContext } from 'svelte';
	import { SvelteFlowProvider } from '@xyflow/svelte';
	import { createId } from '@paralleldrive/cuid2';
	import type { NodeKind } from '$lib/modules/pipeline/types';
	import { PREVIEW_CTX, categoryOrder, categoryLabel, kindsByCategory, registry } from '$lib/modules/flow/utils';
	import { startFakeSignal } from './_fake_signal';

	setContext(PREVIEW_CTX, true);

	const ids: Partial<Record<NodeKind, string>> = {};
	function idFor(kind: NodeKind): string {
		return (ids[kind] ??= createId());
	}

	$effect(() =>
		startFakeSignal({
			levelMeter: idFor('levelMeter'),
			lufsMeter: idFor('lufsMeter'),
			waveform: idFor('waveform'),
			spectrum: idFor('spectrum')
		})
	);

	const DATA_OVERRIDES: Partial<Record<NodeKind, Record<string, unknown>>> = {
		microphone: { deviceId: 'splitwave' },
		speaker: { deviceId: 'splitwave' }
	};

	function dataFor(kind: NodeKind): Record<string, unknown> {
		return { ...registry[kind].defaultData, ...(DATA_OVERRIDES[kind] ?? {}) };
	}
</script>

<SvelteFlowProvider>
	<div class="flex flex-col gap-10 p-10">
		{#each categoryOrder as cat (cat)}
			<section class="flex flex-col gap-3">
				<h2 class="text-xs font-semibold tracking-wider text-neutral-900 uppercase">
					{categoryLabel[cat]}
				</h2>
				<div class="flex flex-wrap items-start gap-6">
					{#each kindsByCategory[cat] as kind (kind)}
						{@const Comp = registry[kind].component}
						<div data-node-kind={kind}>
							<Comp id={idFor(kind)} data={dataFor(kind)} />
						</div>
					{/each}
				</div>
			</section>
		{/each}
	</div>
</SvelteFlowProvider>
