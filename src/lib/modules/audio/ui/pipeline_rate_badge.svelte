<script lang="ts">
	import { appSettings } from '$lib/modules/settings/stores.svelte';
	import { Pulse } from '$lib/components/icons';
	import { formatHz } from '$lib/components/format';
	import { currentLocale, t } from '$lib/modules/i18n';

	let rateFormatted = $derived(formatHz(appSettings.pipelineSampleRate));
	let title = $derived(t('audio.pipelineRateTooltip', { hz: appSettings.pipelineSampleRate.toLocaleString(currentLocale()) }));
</script>

<span class="flex items-center gap-1.5 rounded-md border border-theme/10 bg-background px-2 py-0.5" {title}>
	<Pulse class="h-3.5 w-3.5 text-neutral-500" />
	<span class="font-mono text-xs text-neutral-800 tabular-nums">{rateFormatted}</span>
</span>
