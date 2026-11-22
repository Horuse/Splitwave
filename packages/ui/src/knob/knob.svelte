<script lang="ts">
	import { onDestroy } from 'svelte';

	interface Props {
		label: string;
		value: number;
		min: number;
		max: number;
		step?: number;
		format?: (v: number) => string;
		/** Double-click resets to this value. */
		defaultValue?: number;
		class?: string;
		onChange: (v: number) => void;
	}

	let { label, value, min, max, step = 0, format, defaultValue, class: className, onChange }: Props = $props();

	// Pixels of vertical drag for the full range; Shift drags ten times finer.
	const TRAVEL = 160;
	// The arc opens at the bottom: 270 degrees from -135 to +135.
	const SWEEP = 270;
	const R = 15;
	const C = 2 * Math.PI * R;
	const ARC = (C * SWEEP) / 360;

	let ghost = $state<number | null>(null);
	let shown = $derived(ghost ?? value);
	let t = $derived(max > min ? (shown - min) / (max - min) : 0);
	let angle = $derived(-SWEEP / 2 + t * SWEEP);
	let text = $derived(format ? format(shown) : shown.toFixed(2));

	function clamp(v: number): number {
		const s = step > 0 ? Math.round((v - min) / step) * step + min : v;
		return Math.min(max, Math.max(min, s));
	}

	let startY = 0;
	let startValue = 0;
	let raf: number | undefined;

	function emit(v: number) {
		ghost = v;
		if (raf !== undefined) return;
		raf = requestAnimationFrame(() => {
			raf = undefined;
			if (ghost !== null) onChange(ghost);
		});
	}

	function onPointerDown(e: PointerEvent) {
		if (e.button !== 0) return;
		e.preventDefault();
		(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
		(e.currentTarget as HTMLElement).focus();
		startY = e.clientY;
		startValue = value;
		ghost = value;
	}

	function onPointerMove(e: PointerEvent) {
		if (ghost === null) return;
		const range = (max - min) * (e.shiftKey ? 0.1 : 1);
		emit(clamp(startValue + ((startY - e.clientY) / TRAVEL) * range));
	}

	function onPointerUp() {
		if (ghost === null) return;
		if (raf !== undefined) cancelAnimationFrame(raf);
		raf = undefined;
		const v = ghost;
		ghost = null;
		if (v !== value) onChange(v);
	}

	function onKeyDown(e: KeyboardEvent) {
		const unit = step > 0 ? step : (max - min) / 100;
		const delta = { ArrowUp: unit, ArrowRight: unit, ArrowDown: -unit, ArrowLeft: -unit }[e.key];
		if (delta === undefined) return;
		e.preventDefault();
		onChange(clamp(value + delta * (e.shiftKey ? 10 : 1)));
	}

	onDestroy(() => {
		if (raf !== undefined) cancelAnimationFrame(raf);
	});
</script>

<div class={['flex min-w-0 flex-col items-center gap-0.5', className]}>
	<div
		role="slider"
		tabindex="0"
		aria-label={label}
		aria-valuemin={min}
		aria-valuemax={max}
		aria-valuenow={shown}
		aria-valuetext={text}
		title={defaultValue !== undefined ? 'Drag up or down. Shift for fine steps, double-click to reset' : 'Drag up or down. Shift for fine steps'}
		class="size-11 cursor-ns-resize touch-none rounded-full outline-none focus-visible:ring-2 focus-visible:ring-theme/30"
		onpointerdown={onPointerDown}
		onpointermove={onPointerMove}
		onpointerup={onPointerUp}
		onpointercancel={onPointerUp}
		ondblclick={() => defaultValue !== undefined && onChange(defaultValue)}
		onkeydown={onKeyDown}>
		<svg viewBox="0 0 40 40" class="size-full" aria-hidden="true">
			<g transform="rotate(135 20 20)">
				<circle cx="20" cy="20" r={R} fill="none" class="stroke-theme/15" stroke-width="3" stroke-linecap="round" stroke-dasharray="{ARC} {C}" />
				<circle cx="20" cy="20" r={R} fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-dasharray="{ARC * t} {C}" />
			</g>
			<circle cx="20" cy="20" r="10" class="fill-neutral-200" />
			<line x1="20" y1="20" x2="20" y2="12" transform="rotate({angle} 20 20)" class="stroke-neutral-1100" stroke-width="2" stroke-linecap="round" />
		</svg>
	</div>
	<span class="max-w-full truncate text-[10px] text-neutral-900" title={label}>{label}</span>
	<span class="font-mono text-[10px] text-neutral-1000 tabular-nums">{text}</span>
</div>
