<script lang="ts">
	import { modalManager, type ModalBaseProps } from '$lib/modules/overlay/modal';
	import { registry } from '$lib/modules/flow/utils/nodes';
	import { Add } from '$lib/components/icons';
	import type { NodeKind } from '$lib/modules/pipeline/types';
	import { TEMPLATES } from '../catalog';
	import type { Template, TemplateAccent } from '../types';

	let { modalId }: ModalBaseProps = $props();

	// Written out in full: Tailwind only ships classes it can see as literals.
	const CARD: Record<TemplateAccent, string> = {
		neutral: 'border-neutral-400 bg-neutral-200/60 hover:bg-neutral-300/60',
		emerald: 'border-emerald-500/30 bg-emerald-500/10 hover:bg-emerald-500/20',
		sky: 'border-sky-500/30 bg-sky-500/10 hover:bg-sky-500/20',
		violet: 'border-violet-500/30 bg-violet-500/10 hover:bg-violet-500/20',
		amber: 'border-amber-500/30 bg-amber-500/10 hover:bg-amber-500/20',
		rose: 'border-rose-500/30 bg-rose-500/10 hover:bg-rose-500/20'
	};

	const TILE: Record<TemplateAccent, string> = {
		neutral: 'bg-neutral-300/70 text-neutral-1000',
		emerald: 'bg-emerald-500/15 text-emerald-700 dark:text-emerald-300',
		sky: 'bg-sky-500/15 text-sky-700 dark:text-sky-300',
		violet: 'bg-violet-500/15 text-violet-700 dark:text-violet-300',
		amber: 'bg-amber-500/15 text-amber-700 dark:text-amber-300',
		rose: 'bg-rose-500/15 text-rose-700 dark:text-rose-300'
	};

	/** One chip per distinct kind, so a two-mic template shows Microphone once. */
	function kinds(t: Template): NodeKind[] {
		return [...new Set(t.nodes.map((n) => n.kind))];
	}

	function pick(t: Template) {
		modalManager.close(modalId, t.id);
	}
</script>

<div class="grid grid-cols-2 gap-3 px-5 pt-1 pb-5 sm:grid-cols-3">
	{#each TEMPLATES as t (t.id)}
		<button
			type="button"
			onclick={() => pick(t)}
			class={[
				'flex h-full min-h-44 flex-col items-center justify-center gap-3 rounded-xl border p-4 text-center transition-colors',
				CARD[t.accent]
			]}
		>
			<div class="flex min-h-9 flex-wrap items-center justify-center gap-1.5">
				{#if t.nodes.length === 0}
					<span
						class={[
							'flex size-9 items-center justify-center rounded-lg border border-dashed border-neutral-500',
							TILE[t.accent]
						]}
					>
						<Add class="size-4" />
					</span>
				{:else}
					{#each kinds(t) as kind (kind)}
						{@const entry = registry[kind]}
						<span
							class={['flex size-9 items-center justify-center rounded-lg', TILE[t.accent]]}
							title={entry.label}
						>
							<entry.icon class="size-4" />
						</span>
					{/each}
				{/if}
			</div>

			<div class="flex flex-col gap-1">
				<span class="text-sm font-medium text-theme">{t.name}</span>
				<span class="text-[11px] leading-snug text-neutral-900">{t.description}</span>
			</div>
		</button>
	{/each}
</div>
