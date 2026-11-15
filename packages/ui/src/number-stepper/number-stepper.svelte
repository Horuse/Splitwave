<script lang="ts">
	import IconAdd from '~icons/fluent/add-12-regular';
	import IconMinus from '~icons/fluent/subtract-12-regular';

	let {
		value,
		min,
		max,
		step = 1,
		disabled = false,
		label = 'Value',
		width = 'w-12',
		class: className,
		onchange
	}: {
		value: number;
		min: number;
		max: number;
		step?: number;
		disabled?: boolean;
		label?: string;
		width?: string;
		class?: string;
		onchange: (value: number) => void;
	} = $props();

	// Local text so typing is not overwritten by reactive updates; re-syncs when the external value changes.
	let raw = $state(String(value));
	$effect(() => {
		raw = String(value);
	});

	function commit() {
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

<div class={['stepper', className]}>
	<button
		type="button"
		class="stepper-btn"
		disabled={disabled || value <= min}
		onclick={() => onchange(Math.max(min, value - step))}
		aria-label="Decrease {label}">
		<IconMinus class="size-2.5" />
	</button>
	<input type="number" class="stepper-input {width}" {disabled} {min} {max} {step} bind:value={raw} onchange={commit} aria-label={label} />
	<button
		type="button"
		class="stepper-btn"
		disabled={disabled || value >= max}
		onclick={() => onchange(Math.min(max, value + step))}
		aria-label="Increase {label}">
		<IconAdd class="size-2.5" />
	</button>
</div>
