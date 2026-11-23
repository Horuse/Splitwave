<script lang="ts">
	import { setContext, untrack } from 'svelte';
	import { SvelteFlowProvider } from '@xyflow/svelte';
	import type { NodeKind } from '@splitwave/nodes';
	import { PREVIEW_CTX } from '$lib/modules/flow/utils';
	import { categoryLabel, categoryOrder, kindsByCategory, nodeTypes, registry } from '$lib/modules/flow/utils/nodes';
	import { startSignal, type SignalOptions } from './signal';
	import { createDemoPort, setEnginePort, startSignal as startDemoSignal } from '@splitwave/nodes/engine';
	import { appEnginePort } from '$lib/modules/audio/engine_port.svelte';

	/** `demo` renders through the package's own port, as the landing page will: no Tauri, no app stores. */
	let { signal, demo = false }: { signal: SignalOptions; demo?: boolean } = $props();

	const demoPort = createDemoPort();
	// The port is fixed for the component's life, like a host's.
	setEnginePort(untrack(() => demo) ? demoPort : appEnginePort);

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
			.map((kind) => {
				const targets = { nodeId: idFor(kind), wideScope: kind === 'spectrum', running: false };
				return demo ? startDemoSignal(targets, signal, demoPort.emit) : startSignal(targets, signal);
			});
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
						{@const Comp = nodeTypes[kind]}
						<div data-node-kind={kind}>
							<Comp id={idFor(kind)} data={dataFor(kind)} />
						</div>
					{/each}
				</div>
			</section>
		{/each}
	</div>
</SvelteFlowProvider>
