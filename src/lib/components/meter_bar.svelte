<script lang="ts">
	import type { Snippet } from 'svelte';

	let {
		pct,
		gradient,
		orientation = 'horizontal',
		fillClass = '',
		trackClass = '',
		ghost = false,
		hold = null,
		hover = false,
		hoverLabel,
		class: klass = '',
		children
	}: {
		pct: number;
		gradient?: string;
		orientation?: 'horizontal' | 'vertical';
		fillClass?: string;
		trackClass?: string;
		ghost?: boolean;
		hold?: number | null;
		hover?: boolean;
		hoverLabel?: (pct: number) => string;
		class?: string;
		children?: Snippet;
	} = $props();

	// clip-path reveals the fill from left (horizontal) or bottom (vertical).
	let clip = $derived(
		orientation === 'vertical' ? `inset(${100 - pct}% 0 0 0)` : `inset(0 ${100 - pct}% 0 0)`
	);
	let holdStyle = $derived(
		orientation === 'vertical'
			? `left: 0; right: 0; height: 1px; bottom: ${hold}%;`
			: `top: 0; bottom: 0; width: 1px; left: ${hold}%;`
	);

	let hoverPct = $state<number | null>(null);
	let hoverPos = $state(0); // px along the reveal axis

	// Divide out the xyflow zoom so the reading tracks the cursor at any scale.
	function onMove(e: MouseEvent) {
		const el = e.currentTarget as HTMLElement;
		const rect = el.getBoundingClientRect();
		if (orientation === 'vertical') {
			const scale = rect.height / el.offsetHeight;
			const y = (e.clientY - rect.top) / scale;
			hoverPos = y;
			hoverPct = Math.max(0, Math.min(100, 100 - (y / el.offsetHeight) * 100));
		} else {
			const scale = rect.width / el.offsetWidth;
			const x = (e.clientX - rect.left) / scale;
			hoverPos = x;
			hoverPct = Math.max(0, Math.min(100, (x / el.offsetWidth) * 100));
		}
	}

	let hoverLineStyle = $derived(
		orientation === 'vertical'
			? `left: 0; right: 0; height: 1px; top: ${hoverPos}px;`
			: `top: 0; bottom: 0; width: 1px; left: ${hoverPos}px;`
	);
</script>

<div
	class={['relative overflow-hidden', trackClass, klass]}
	onmousemove={hover ? onMove : undefined}
	onmouseleave={hover ? () => (hoverPct = null) : undefined}
	role="presentation"
>
	{#if ghost && gradient}
		<div class="absolute inset-0 opacity-30 dark:brightness-[0.2]" style="background: {gradient};"></div>
	{/if}
	<div class={['absolute inset-0', fillClass]} style="background: {gradient}; clip-path: {clip};"></div>
	{#if hold != null && hold > 0}
		<div class="absolute bg-white" style={holdStyle}></div>
	{/if}
	{@render children?.()}
	{#if hover && hoverPct !== null}
		<div class="pointer-events-none absolute z-10 bg-cyan-400" style={hoverLineStyle}>
			{#if hoverLabel}
				<span
					class="absolute left-1/2 -translate-x-1/2 whitespace-nowrap rounded bg-neutral-800 px-1 font-mono text-[8px] leading-tight text-white"
					style={orientation === 'vertical' ? `top: ${hoverPos < 12 ? '2px' : '-10px'};` : 'top: -10px;'}
				>{hoverLabel(hoverPct)}</span>
			{/if}
		</div>
	{/if}
</div>
