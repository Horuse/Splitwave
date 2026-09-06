<script lang="ts">
	import { appSettings } from '$lib/modules/settings/stores.svelte';
	import { Pulse } from '$lib/components/icons';

	function formatRate(hz: number): string {
		return hz >= 1000 ? `${(hz / 1000).toFixed(hz % 1000 === 0 ? 0 : 1)} kHz` : `${hz} Hz`;
	}

	let rateFormatted = $derived(formatRate(appSettings.pipelineSampleRate));
</script>

<span
	class="flex items-center gap-1.5 rounded-md border border-theme/10 bg-background px-2 py-0.5"
	title="Pipeline sample rate: {appSettings.pipelineSampleRate.toLocaleString()} Hz. Internal effects, mixing, and routing process at this rate.">
	<Pulse class="h-3.5 w-3.5 text-neutral-500" />
	<span class="font-mono text-xs text-neutral-800 tabular-nums">{rateFormatted}</span>
</span>
