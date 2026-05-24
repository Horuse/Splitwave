<script lang="ts">
	import { setContext } from 'svelte';
	import { SvelteFlowProvider } from '@xyflow/svelte';
	import { createId } from '@paralleldrive/cuid2';
	import type { NodeKind } from '$lib/modules/pipeline/types';
	import {
		PREVIEW_CTX,
		categoryOrder,
		categoryLabel,
		kindsByCategory,
		registry
	} from '$lib/modules/flow/utils';

	setContext(PREVIEW_CTX, true);

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
				<h2 class="text-xs font-semibold uppercase tracking-wider text-neutral-900">
					{categoryLabel[cat]}
				</h2>
				<div class="flex flex-wrap items-start gap-6">
					{#each kindsByCategory[cat] as kind (kind)}
						{@const Comp = registry[kind].component}
						<Comp id={createId()} data={dataFor(kind)} />
					{/each}
				</div>
			</section>
		{/each}
	</div>
</SvelteFlowProvider>
