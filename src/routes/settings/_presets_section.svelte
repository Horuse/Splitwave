<script lang="ts">
	import { methods as presetMethods } from '$lib/modules/preset';
	import type { Preset } from '$lib/modules/preset';
	import { registry } from '$lib/modules/flow/utils/nodes';
	import { modalManager } from '$lib/modules/overlay/modal';
	import { ConfirmModal } from '$lib/modules/overlay/ui';
	import { Delete } from '$lib/components/icons';

	let presets = $state<Preset[]>([]);
	let editing = $state<string | null>(null);
	let draft = $state('');

	async function reload() {
		presets = await presetMethods.listMine();
	}
	$effect(() => {
		reload();
	});

	function startEdit(p: Preset) {
		editing = p.id;
		draft = p.name;
	}

	async function commitEdit(p: Preset) {
		editing = null;
		const name = draft.trim();
		if (!name || name === p.name) return;
		await presetMethods.rename(p.id, name);
		await reload();
	}

	function onKeyDown(e: KeyboardEvent) {
		const el = e.currentTarget as HTMLInputElement;
		// Blur commits; Escape clears the draft first so the blur is a no-op.
		if (e.key === 'Enter') {
			el.blur();
		} else if (e.key === 'Escape') {
			editing = null;
			el.blur();
		}
	}

	async function remove(p: Preset) {
		const ok = await modalManager.open<boolean>(`Delete "${p.name}"?`, ConfirmModal, {
			size: 'sm',
			message: 'Nodes already using it keep their current settings.',
			confirmLabel: 'Delete',
			danger: true
		});
		if (!ok) return;
		await presetMethods.remove(p.id);
		await reload();
	}
</script>

<section class="flex flex-col gap-3">
	<div>
		<h2 class="text-sm font-semibold text-theme">Effect presets</h2>
		<p class="text-xs text-neutral-900">Presets you saved from a node. Built-in ones are not listed -- they cannot be changed.</p>
	</div>

	{#if presets.length === 0}
		<div class="rounded-xl border border-dashed border-neutral-400 px-4 py-8 text-center">
			<p class="text-xs text-neutral-900">No saved presets yet. Use the + button on an effect to store its settings.</p>
		</div>
	{:else}
		<ul class="flex flex-col gap-1.5">
			{#each presets as p (p.id)}
				{@const entry = registry[p.kind]}
				<li class="flex items-center gap-3 rounded-xl border border-neutral-400 bg-neutral-100 px-3 py-2">
					<span class="flex size-7 shrink-0 items-center justify-center rounded-lg bg-neutral-200 text-neutral-1000" title={entry.label}>
						<entry.icon class="size-3.5" />
					</span>

					<div class="flex min-w-0 flex-1 flex-col">
						{#if editing === p.id}
							<!-- svelte-ignore a11y_autofocus -->
							<input
								bind:value={draft}
								autofocus
								onblur={() => commitEdit(p)}
								onkeydown={onKeyDown}
								class="w-full rounded-md border border-neutral-500 bg-neutral-200 px-1.5 py-0.5 text-sm text-theme focus:outline-none" />
						{:else}
							<button
								type="button"
								class="truncate text-left text-sm font-medium text-theme hover:underline"
								title="Rename"
								onclick={() => startEdit(p)}>
								{p.name}
							</button>
						{/if}
						<span class="text-[11px] text-neutral-900">{entry.label}</span>
					</div>

					<button
						type="button"
						class="flex size-7 shrink-0 items-center justify-center rounded-lg text-neutral-1000 transition-colors hover:bg-neutral-300"
						aria-label="Delete preset"
						onclick={() => remove(p)}>
						<Delete class="size-3.5" />
					</button>
				</li>
			{/each}
		</ul>
	{/if}
</section>
