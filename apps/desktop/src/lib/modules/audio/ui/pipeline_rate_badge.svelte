<script lang="ts">
	import { smoothRadius } from '@splitwave/ui';
	import { appSettings } from '$lib/modules/settings/stores.svelte';
	import Pulse from '~icons/fluent/pulse-20-filled';
	import { formatHz } from '@splitwave/nodes';

	let rateFormatted = $derived(formatHz(appSettings.pipelineSampleRate));
</script>

<span
	class="flex h-7 items-center gap-1.5 border border-(--lv-3) bg-(--lv-1) px-2.5"
	style="border-radius: var(--r, 10px)"
	{@attach smoothRadius({ variable: '--r' })}
	title="Pipeline sample rate: {appSettings.pipelineSampleRate.toLocaleString()} Hz. Internal effects, mixing, and routing process at this rate.">
	<Pulse class="size-4 text-neutral-900" />
	<span class="text-xs text-neutral-1100 tabular-nums">{rateFormatted}</span>
</span>
