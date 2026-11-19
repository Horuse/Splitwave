<script lang="ts">
	import type { Attachment } from 'svelte/attachments';

	let { source }: { source: string } = $props();

	let failed = $state(false);

	// pele renders into the element, sized to its width; the library loads only
	// when a diagram is on screen. Its output is escaped SVG, so remote text is safe.
	const diagram: Attachment<HTMLElement> = (el) => {
		let mounted: { destroy(): void } | undefined;
		let live = true;
		failed = false;
		import('pele/lazy')
			.then(({ mountAsync }) => mountAsync(el, source, { onError: () => (failed = true) }))
			.then((m) => {
				if (live) mounted = m;
				else m.destroy();
			})
			.catch(() => (failed = true));
		return () => {
			live = false;
			mounted?.destroy();
		};
	};
</script>

{#if failed}
	<pre class="overflow-x-auto rounded-lg border border-theme/5 bg-neutral-100 p-3 font-mono text-[0.85em] text-neutral-1000">{source}</pre>
	<p class="mt-1 text-xs text-neutral-700">This diagram could not be drawn; its source is shown instead.</p>
{:else}
	{#key source}
		<div
			class="overflow-x-auto rounded-lg border border-theme/5 bg-neutral-100 p-3 [--pele-accent:var(--color-signal)] [--pele-bg:transparent] [--pele-border:var(--color-neutral-400)] [--pele-fg:var(--color-neutral-1100)] [--pele-line:var(--color-neutral-800)] [--pele-muted:var(--color-neutral-700)] [--pele-surface:var(--color-neutral-200)] [--pele-surface-alt:var(--color-neutral-300)]"
			{@attach diagram}>
		</div>
	{/key}
{/if}
