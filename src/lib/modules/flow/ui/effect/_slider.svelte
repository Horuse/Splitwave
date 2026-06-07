<script lang="ts">
	import { onDestroy } from 'svelte';

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
		/** CSS class applied to the numeric readout — lets effects colour-code by value. */
		valueClass?: string;
		onChange: (v: number) => void;
	}

	let {
		label,
		value,
		min,
		max,
		step = 0.1,
		unit = '',
		format,
		defaultValue,
		ticks,
		valueClass = 'text-neutral-900',
		onChange
	}: Props = $props();

	let ghost = $state<number | null>(null);

	let editing = $state(false);
	let draft = $state('');

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

	function display(v: number): string {
		if (format) return format(v);
		const fixed = step >= 1 ? 0 : 1;
		return `${v.toFixed(fixed)}${unit}`;
	}

	function clamp(v: number): number {
		return Math.max(min, Math.min(max, v));
	}

	function leftPct(v: number): number {
		return ((v - min) / (max - min)) * 100;
	}

	function onInput(e: Event) {
		const next = Number((e.currentTarget as HTMLInputElement).value);
		if (!Number.isNaN(next)) {
			ghost = next;
			scheduleEmit(next);
		}
	}

	function clearGhost() {
		flushEmit();
		ghost = null;
	}

	onDestroy(() => {
		if (pendingRaf !== undefined) cancelAnimationFrame(pendingRaf);
	});

	function onDoubleClick(e: MouseEvent) {
		if (defaultValue === undefined) return;
		e.preventDefault();
		onChange(clamp(defaultValue));
	}

	// Replace native <input range> arrow stepping so Shift = fine, Alt = coarse.
	function onKeyDown(e: KeyboardEvent) {
		const dir = e.key === 'ArrowLeft' || e.key === 'ArrowDown'
			? -1
			: e.key === 'ArrowRight' || e.key === 'ArrowUp'
				? 1
				: 0;
		if (dir !== 0) {
			e.preventDefault();
			const mul = e.altKey ? 10 : e.shiftKey ? 0.1 : 1;
			onChange(clamp(value + dir * step * mul));
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

<label class="flex flex-col gap-0.5 text-[11px] text-neutral-1000">
	<span class="flex items-baseline justify-between">
		<span>{label}</span>
		<input
			type="text"
			inputmode="decimal"
			class="nodrag nopan w-14 rounded bg-transparent text-right font-mono tabular-nums focus:bg-neutral-100 focus:outline-none focus:ring-1 focus:ring-amber-500 {valueClass}"
			value={editing ? draft : display(value)}
			oninput={(e) => (draft = e.currentTarget.value)}
			onfocus={startEdit}
			onblur={commitEdit}
			onkeydown={onEditKeyDown}
		/>
	</span>
	<div class="relative pt-1 pb-2">
		<input
			type="range"
			class="nodrag nopan nowheel w-full accent-amber-500"
			{min}
			{max}
			{step}
			{value}
			oninput={onInput}
			onpointerup={clearGhost}
			onpointercancel={clearGhost}
			onblur={clearGhost}
			ondblclick={onDoubleClick}
			onkeydown={onKeyDown}
		/>
		{#if ticks && ticks.length > 0}
			<div class="pointer-events-none absolute inset-x-0 bottom-0 h-1.5">
				{#each ticks as t (t)}
					<span
						class="absolute top-0 h-1.5 w-px bg-neutral-700/50"
						style="left: {leftPct(t)}%;"
					></span>
				{/each}
			</div>
		{/if}
		{#if ghost !== null}
			<span
				class="pointer-events-none absolute -top-0.5 z-10 -translate-x-1/2 rounded bg-neutral-800 px-1 font-mono text-[9px] leading-tight text-white shadow"
				style="left: {leftPct(ghost)}%;"
			>
				{display(ghost)}
			</span>
		{/if}
	</div>
</label>
