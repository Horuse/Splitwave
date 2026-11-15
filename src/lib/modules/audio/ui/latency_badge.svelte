<script lang="ts">
	import { Tooltip, smoothRadius } from '@splitwave/ui';
	import Gauge from '~icons/fluent/gauge-16-filled';
	import { formatLatencyMs as ms } from '@splitwave/nodes';
	import { latencyStore } from '../latency.svelte';

	let report = $derived(latencyStore.report);
	let path = $derived(report?.path ?? null);
	let loadPct = $derived(Math.round((report?.dspLoad ?? 0) * 100));
	let strained = $derived((report?.overloads ?? 0) > 0 || loadPct >= 80);

	let rows = $derived(
		path
			? [
					{ label: 'Input device', value: path.inputDeviceMs, color: 'bg-sky-500' },
					{ label: 'Input queue', value: path.inputQueueMs, color: 'bg-violet-500' },
					{ label: 'Processing', value: path.processingMs, color: 'bg-brand' },
					{ label: 'Output adapter', value: path.outputAdapterMs, color: 'bg-emerald-500' },
					{ label: 'Output device', value: path.outputDeviceMs, color: 'bg-teal-500' }
				].filter((r) => r.value > 0.05)
			: []
	);
	let sum = $derived(rows.reduce((a, r) => a + r.value, 0) || 1);
</script>

{#if report && path}
	<Tooltip placement="bottom">
		<span
			class={['flex h-7 items-center gap-1.5 border px-2.5', strained ? 'border-amber-500/60 bg-amber-500/10' : 'border-(--lv-3) bg-(--lv-1)']}
			style="border-radius: var(--r, 10px)"
			{@attach smoothRadius({ variable: '--r' })}>
			<Gauge class={['size-4', strained ? 'text-amber-600' : 'text-neutral-900']} />
			<span class="text-xs text-neutral-1100 tabular-nums">
				{ms(path.totalMs)}{path.hardwareIncluded ? '' : '+'} ms
			</span>
		</span>
		{#snippet content()}
			<div class="flex w-64 flex-col gap-3 py-1">
				<div class="flex flex-col gap-0.5">
					<span class="text-neutral-900">Round-trip latency</span>
					<span class="text-2xl font-semibold text-theme tabular-nums">
						{ms(path.totalMs)}{path.hardwareIncluded ? '' : '+'}<span class="ml-1 text-sm font-normal text-neutral-900">ms</span>
					</span>
				</div>

				<div class="flex h-1.5 w-full gap-0.5 overflow-hidden rounded-full">
					{#each rows as r (r.label)}
						<span class={['h-full', r.color]} style="width: {(r.value / sum) * 100}%"></span>
					{/each}
				</div>

				<div class="flex flex-col gap-1">
					{#each rows as r (r.label)}
						<span class="flex items-center gap-2 tabular-nums">
							<span class={['size-2 shrink-0 rounded-full', r.color]}></span>
							<span class="flex-1 text-neutral-1000">{r.label}</span>
							<span class="text-neutral-1200">{ms(r.value)} ms</span>
						</span>
					{/each}
				</div>

				<div class="flex flex-col gap-1.5 border-t border-(--lv-3) pt-3">
					<span class="flex items-center justify-between gap-4 tabular-nums">
						<span class="text-neutral-1000">Buffer</span>
						<span class="text-neutral-1200">
							{report.bufferFrames}{report.deviceBufferFrames !== null && report.deviceBufferFrames !== report.bufferFrames
								? ` (device ${report.deviceBufferFrames})`
								: ''} samples
						</span>
					</span>
					<span class="flex items-center gap-2 tabular-nums">
						<span class="text-neutral-1000">DSP load</span>
						<span class="h-1.5 flex-1 overflow-hidden rounded-full bg-(--lv-3)">
							<span class={['block h-full', strained ? 'bg-amber-500' : 'bg-neutral-1000']} style="width: {Math.min(loadPct, 100)}%"></span>
						</span>
						<span class={['w-9 text-right', strained ? 'text-amber-600' : 'text-neutral-1200']}>{loadPct}%</span>
					</span>
				</div>

				{#if report.overloads > 0 || !path.hardwareIncluded}
					<div class="flex flex-col gap-1">
						{#if report.overloads > 0}
							<span class="text-amber-600">Audio dropped: raise the buffer size.</span>
						{/if}
						{#if !path.hardwareIncluded}
							<span class="text-neutral-900">Device hardware latency is not reported by the OS.</span>
						{/if}
					</div>
				{/if}
			</div>
		{/snippet}
	</Tooltip>
{/if}
