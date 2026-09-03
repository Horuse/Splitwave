<script lang="ts">
	import { Add, Minus } from './icons';

	let {
		value,
		min,
		max,
		step = 1,
		disabled = false,
		label = 'Value',
		width = 'w-12',
		onchange
	}: {
		value: number;
		min: number;
		max: number;
		step?: number;
		disabled?: boolean;
		label?: string;
		width?: string;
		onchange: (value: number) => void;
	} = $props();

	// Local text so typing isn't clobbered by reactive updates; re-syncs when
	// the external value changes.
	let raw = $state(String(value));
	$effect(() => {
		raw = String(value);
	});

	function commit() {
		// `type="number"` binds a number, so the text must be coerced.
		const text = String(raw ?? '');
		const n = Math.round(Number(text));
		if (text.trim() === '' || !Number.isFinite(n)) {
			raw = String(value);
			return;
		}
		const next = Math.min(max, Math.max(min, n));
		raw = String(next);
		if (next !== value) onchange(next);
	}
</script>

<div class="nodrag nopan flex items-center overflow-hidden rounded-md border border-neutral-400 bg-neutral-100">
	<button
		type="button"
		class="flex h-6 w-6 items-center justify-center text-neutral-900 hover:bg-neutral-300 disabled:cursor-not-allowed disabled:opacity-40"
		disabled={disabled || value <= min}
		onclick={() => onchange(Math.max(min, value - step))}
		aria-label="Decrease {label}">
		<Minus class="size-2" />
	</button>
	<input
		type="number"
		class="h-6 {width} [appearance:textfield] border-x border-neutral-400 bg-transparent text-center font-mono text-[10px] tabular-nums outline-none disabled:cursor-not-allowed disabled:opacity-40 [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none"
		{disabled}
		{min}
		{max}
		{step}
		bind:value={raw}
		onchange={commit}
		aria-label={label} />
	<button
		type="button"
		class="flex h-6 w-6 items-center justify-center text-neutral-900 hover:bg-neutral-300 disabled:cursor-not-allowed disabled:opacity-40"
		disabled={disabled || value >= max}
		onclick={() => onchange(Math.min(max, value + step))}
		aria-label="Increase {label}">
		<Add class="size-2" />
	</button>
</div>
