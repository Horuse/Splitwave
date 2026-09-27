<script lang="ts">
	import { setContext } from 'svelte';
	import { SvelteFlowProvider } from '@xyflow/svelte';
	import type { NodeKind } from '$lib/modules/pipeline/types';
	import { PREVIEW_CTX, categoryLabel, categoryOrder, kindsByCategory, registry } from '$lib/modules/flow/utils';
	import { startSignal, type SignalOptions } from './signal';

	let { signal }: { signal: SignalOptions } = $props();

	setContext(PREVIEW_CTX, true);

	const idFor = (kind: NodeKind) => `gallery-${kind}`;

	const DATA_OVERRIDES: Partial<Record<NodeKind, Record<string, unknown>>> = {
		microphone: { deviceId: 'in-1' },
		speaker: { deviceId: 'out-1' }
	};

	function dataFor(kind: NodeKind): Record<string, unknown> {
		return { ...structuredClone(registry[kind].defaultData), ...(DATA_OVERRIDES[kind] ?? {}) };
	}

	$effect(() => {
		const stops = categoryOrder
			.flatMap((cat) => kindsByCategory[cat])
			.map((kind) => startSignal({ nodeId: idFor(kind), wideScope: kind === 'spectrum', running: false }, signal));
		return () => stops.forEach((stop) => stop());
	});
</script>

<SvelteFlowProvider>
	<div class="flex flex-col gap-10 p-10">
		{#each categoryOrder as cat (cat)}
			<section class="flex flex-col gap-3">
				<h2 class="text-xs font-semibold tracking-wider text-neutral-900 uppercase">{categoryLabel[cat]}</h2>
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
