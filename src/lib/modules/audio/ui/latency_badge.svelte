<script lang="ts">
	import Gauge from '$lib/components/icons/gauge.svelte';
	import { Tooltip } from '$lib/modules/overlay/ui';
	import { formatLatencyMs as ms } from '$lib/components/format';
	import { latencyStore } from '../latency.svelte';

	let report = $derived(latencyStore.report);
	let path = $derived(report?.path ?? null);
	let loadPct = $derived(Math.round((report?.dspLoad ?? 0) * 100));
	let strained = $derived((report?.overloads ?? 0) > 0 || loadPct >= 80);

	let rows = $derived(
		path
			? [
					['Input device', path.inputDeviceMs],
					['Input queue', path.inputQueueMs],
					['Processing', path.processingMs],
					['Output adapter', path.outputAdapterMs],
					['Output device', path.outputDeviceMs]
				].filter(([, v]) => (v as number) > 0.05)
			: []
	);
</script>

{#if report && path}
	<Tooltip placement="bottom">
		<span
			class={[
				'flex items-center gap-1.5 rounded-md border bg-background px-2 py-0.5',
				strained ? 'border-amber-500' : 'border-theme/10'
			]}>
			<Gauge class={['h-3.5 w-3.5', strained ? 'text-amber-600' : 'text-neutral-500']} />
			<span class="font-mono text-xs text-neutral-800 tabular-nums">
				{ms(path.totalMs)}{path.hardwareIncluded ? '' : '+'} ms
			</span>
		</span>
		{#snippet content()}
			<div class="flex min-w-44 flex-col gap-0.5">
				<span class="text-neutral-1200">Round-trip latency</span>
				{#each rows as [label, value] (label)}
					<span class="flex justify-between gap-4 tabular-nums">
						<span>{label}</span><span>{ms(value as number)} ms</span>
					</span>
				{/each}
				<span class="mt-1 flex justify-between gap-4 tabular-nums">
					<span>Buffer</span>
					<span>
						{report.bufferFrames}{report.deviceBufferFrames !== null && report.deviceBufferFrames !== report.bufferFrames
							? ` (device ${report.deviceBufferFrames})`
							: ''} smp
					</span>
				</span>
				<span class="flex justify-between gap-4 tabular-nums">
					<span>DSP load</span><span class={strained ? 'text-amber-600' : undefined}>{loadPct}%</span>
				</span>
				{#if report.overloads > 0}
					<span class="text-amber-600">Audio dropped: raise the buffer size</span>
				{/if}
				{#if !path.hardwareIncluded}
					<span>Device hardware latency not reported by the OS</span>
				{/if}
			</div>
		{/snippet}
	</Tooltip>
{/if}
