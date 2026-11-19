<script lang="ts">
	import { onDestroy } from 'svelte';
	import Info from '~icons/fluent/info-12-regular';
	import Tooltip from '../overlay/tooltip/tooltip.svelte';

	interface Props {
		label: string;
		value: number;
		min: number;
		max: number;
		step?: number;
		unit?: string;
		format?: (v: number) => string;
		/** Double-clicking the track resets to this value. */
		defaultValue?: number;
		/** Optional canonical positions to render as small tick marks. */
		ticks?: number[];
		/** What the parameter does here; shows an info mark beside the label. */
		info?: string;
		/** Colours the numeric readout so an effect can flag a risky value. */
		tone?: 'default' | 'success' | 'warning' | 'danger';
		class?: string;
		onChange: (v: number) => void;
	}

	let { label, value, min, max, step = 0.1, unit = '', format, defaultValue, ticks, info, tone = 'default', class: className, onChange }: Props = $props();

	const TONE = { default: 'text-neutral-900', success: 'text-emerald-700', warning: 'text-amber-600', danger: 'text-red-500' } as const;
	let valueClass = $derived(TONE[tone]);

	/** Value under the pointer while dragging; the engine hears it once per frame. */
	let ghost = $state<number | null>(null);
	let shown = $derived(ghost ?? value);

	let editing = $state(false);
	let draft = $state('');

	let rail: HTMLDivElement | undefined = $state();

	function startEdit(e: FocusEvent) {
		editing = true;
		draft = String(value);
		(e.currentTarget as HTMLInputElement).select();
	}

	function commitEdit() {
		editing = false;
		const next = Number(draft);
		if (!Number.isNaN(next)) onChange(clamp(next));
	}

	function onEditKeyDown(e: KeyboardEvent) {
		if (e.key === 'Enter') {
			e.preventDefault();
			(e.currentTarget as HTMLInputElement).blur();
		} else if (e.key === 'Escape') {
			e.preventDefault();
			editing = false;
			(e.currentTarget as HTMLInputElement).blur();
		}
	}

	let pendingValue: number | null = null;
	let pendingRaf: number | undefined;

	function scheduleEmit(v: number) {
		pendingValue = v;
		if (pendingRaf !== undefined) return;
		pendingRaf = requestAnimationFrame(() => {
			pendingRaf = undefined;
			const next = pendingValue;
			pendingValue = null;
			if (next !== null) onChange(next);
		});
	}

	function flushEmit() {
		if (pendingRaf === undefined) return;
		cancelAnimationFrame(pendingRaf);
		pendingRaf = undefined;
		if (pendingValue !== null) {
			const next = pendingValue;
			pendingValue = null;
			onChange(next);
		}
	}

	onDestroy(() => {
		if (pendingRaf !== undefined) cancelAnimationFrame(pendingRaf);
	});

	function display(v: number): string {
		if (format) return format(v);
		const fixed = step >= 1 ? 0 : 1;
		return `${v.toFixed(fixed)}${unit}`;
	}

	function clamp(v: number): number {
		return Math.max(min, Math.min(max, v));
	}

	function snap(v: number): number {
		const n = Math.round((v - min) / step) * step + min;
		// Strip float noise so 0.1 steps read 0.3, not 0.30000000000000004.
		return clamp(Number(n.toFixed(6)));
	}

	function pct(v: number): number {
		return ((clamp(v) - min) / (max - min)) * 100;
	}

	function valueAt(clientX: number): number {
		const r = rail!.getBoundingClientRect();
		const ratio = r.width > 0 ? (clientX - r.left) / r.width : 0;
		return snap(min + Math.max(0, Math.min(1, ratio)) * (max - min));
	}

	function move(v: number) {
		if (v === (ghost ?? value)) return;
		ghost = v;
		scheduleEmit(v);
	}

	function onPointerDown(e: PointerEvent) {
		if (e.button !== 0 || !rail) return;
		e.preventDefault();
		(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
		(e.currentTarget as HTMLElement).querySelector<HTMLElement>('[role=slider]')?.focus();
		move(valueAt(e.clientX));
	}

	function onPointerMove(e: PointerEvent) {
		if (!(e.currentTarget as HTMLElement).hasPointerCapture(e.pointerId)) return;
		move(valueAt(e.clientX));
	}

	function release() {
		flushEmit();
		ghost = null;
	}

	function onDoubleClick(e: MouseEvent) {
		if (defaultValue === undefined) return;
		e.preventDefault();
		onChange(clamp(defaultValue));
	}

	// Shift = fine, Alt = coarse.
	function onKeyDown(e: KeyboardEvent) {
		const dir = e.key === 'ArrowLeft' || e.key === 'ArrowDown' ? -1 : e.key === 'ArrowRight' || e.key === 'ArrowUp' ? 1 : 0;
		if (dir !== 0) {
			e.preventDefault();
			const mul = e.altKey ? 10 : e.shiftKey ? 0.1 : 1;
			onChange(clamp(Number((value + dir * step * mul).toFixed(6))));
			return;
		}
		if (e.key === 'Home') {
			e.preventDefault();
			onChange(min);
		} else if (e.key === 'End') {
			e.preventDefault();
			onChange(max);
		}
	}
</script>

<div class={['flex flex-col gap-1 text-[11px] text-neutral-1000', className]}>
	<span class="flex items-baseline justify-between">
		<span class="flex items-center gap-1">
			{label}
			{#if info}
				<Tooltip>
					<span class="flex cursor-help text-neutral-600 hover:text-neutral-900" aria-label="About {label}">
						<Info class="size-3" />
					</span>
					{#snippet content()}
						<span class="block max-w-56 leading-snug">{info}</span>
					{/snippet}
				</Tooltip>
			{/if}
		</span>
		<input
			type="text"
			inputmode="decimal"
			aria-label="{label} value"
			class="nodrag nopan w-14 rounded bg-transparent text-right font-mono tabular-nums focus:bg-(--lv-1) focus:ring-1 focus:ring-(--lv-4) focus:outline-none {valueClass}"
			value={editing ? draft : display(value)}
			oninput={(e) => (draft = e.currentTarget.value)}
			onfocus={startEdit}
			onblur={commitEdit}
			onkeydown={onEditKeyDown} />
	</span>

	<!-- The rail is inset by the thumb radius, so the thumb, fill, ticks and
	     tooltip all share one coordinate space and the thumb never overhangs. -->
	<div
		class={['nodrag nopan nowheel relative h-4 cursor-pointer touch-none select-none', ticks?.length && 'mb-2']}
		onpointerdown={onPointerDown}
		onpointermove={onPointerMove}
		onpointerup={release}
		onpointercancel={release}
		ondblclick={onDoubleClick}
		role="presentation">
		<div bind:this={rail} class="absolute inset-x-[7px] inset-y-0">
			<div class="absolute inset-x-[-7px] top-1/2 h-1 -translate-y-1/2 rounded-full bg-(--lv-3)"></div>
			<div class="absolute top-1/2 left-[-7px] h-1 -translate-y-1/2 rounded-full bg-neutral-900" style="width: calc({pct(shown)}% + 7px);"></div>

			{#if ticks?.length}
				{#each ticks as t (t)}
					<span class="absolute top-full mt-1 h-1.5 w-px -translate-x-1/2 bg-neutral-700/50" style="left: {pct(t)}%;"></span>
				{/each}
			{/if}

			<div
				role="slider"
				tabindex="0"
				aria-label={label}
				aria-valuemin={min}
				aria-valuemax={max}
				aria-valuenow={value}
				aria-valuetext={display(value)}
				class="absolute top-1/2 size-3.5 -translate-x-1/2 -translate-y-1/2 rounded-full border border-neutral-400 bg-white outline-none"
				style="left: {pct(shown)}%;"
				onkeydown={onKeyDown}
				onblur={release}>
			</div>

			{#if ghost !== null}
				<span
					class="pointer-events-none absolute bottom-full z-[60] mb-1.5 -translate-x-1/2 rounded-md border border-theme/5 bg-(--lv-2) px-1.5 py-0.5 font-mono text-[10px] leading-tight whitespace-nowrap text-neutral-1000 tabular-nums"
					style="left: {pct(ghost)}%;">
					{display(ghost)}
				</span>
			{/if}
		</div>
	</div>
</div>
