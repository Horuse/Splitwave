<script lang="ts">
	import NumberFlow, { NumberFlowGroup } from '@number-flow/svelte';
	import type { ClassValue } from 'svelte/elements';
	import { numberAnimations } from './config.svelte';

	interface Props {
		/** Duration in milliseconds. */
		value: number;
		/** Show milliseconds after seconds. */
		milliseconds?: boolean;
		class?: ClassValue;
	}

	let { value, milliseconds = false, class: className }: Props = $props();

	const SECOND = 1000;
	const MINUTE = 60 * SECOND;
	const HOUR = 60 * MINUTE;
	const DAY = 24 * HOUR;

	let parts = $derived({
		days: Math.floor(value / DAY),
		hours: Math.floor((value % DAY) / HOUR),
		minutes: Math.floor((value % HOUR) / MINUTE),
		seconds: Math.floor((value % MINUTE) / SECOND),
		ms: Math.floor(value % SECOND)
	});
</script>

<!-- Larger units appear only once reached, so a short run reads "12s", not "0d 0h 0m 12s". -->
<NumberFlowGroup>
	<span class={['inline-flex items-center gap-[0.35em] tabular-nums', className]}>
		{#if parts.days > 0}
			<span class="inline-flex items-center"><NumberFlow locales="en-US" animated={numberAnimations.enabled} value={parts.days} />d</span>
		{/if}
		{#if parts.days > 0 || parts.hours > 0}
			<span class="inline-flex items-center"><NumberFlow locales="en-US" animated={numberAnimations.enabled} value={parts.hours} />h</span>
		{/if}
		{#if parts.days > 0 || parts.hours > 0 || parts.minutes > 0}
			<span class="inline-flex items-center"><NumberFlow locales="en-US" animated={numberAnimations.enabled} value={parts.minutes} />m</span>
		{/if}
		<span class="inline-flex items-center"><NumberFlow locales="en-US" animated={numberAnimations.enabled} value={parts.seconds} />s</span>
		{#if milliseconds}
			<span class="inline-flex items-center"><NumberFlow locales="en-US" animated={numberAnimations.enabled} value={parts.ms} />ms</span>
		{/if}
	</span>
</NumberFlowGroup>
