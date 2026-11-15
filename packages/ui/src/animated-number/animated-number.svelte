<script lang="ts">
	import NumberFlow from '@number-flow/svelte';
	import type { ClassValue } from 'svelte/elements';
	import { numberAnimations } from './config.svelte';

	interface Props {
		value: number;
		prefix?: string;
		suffix?: string;
		/** Intl.NumberFormat options: fraction digits, compact notation. */
		format?: Intl.NumberFormatOptions;
		class?: ClassValue;
	}

	let { value, prefix, suffix, format = { maximumFractionDigits: 0 }, class: className }: Props = $props();
</script>

<NumberFlow class={['tabular-nums', className]} locales="en-US" animated={numberAnimations.enabled} {value} {prefix} {suffix} {format} />
