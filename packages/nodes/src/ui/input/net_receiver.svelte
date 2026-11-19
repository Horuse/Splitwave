<script lang="ts">
	import { SignalBars } from '@splitwave/ui';
	import { onDestroy, untrack } from 'svelte';
	import { useNodeConnections, useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { NetReceiverNodeData } from '@splitwave/nodes';
	import { formatHz, formatRate, formatPct, LossWindow } from '@splitwave/nodes';
	import Wrapper from '../node.svelte';
	import ArrowDownload from '~icons/fluent/arrow-download-16-filled';
	import { CAPTION, FIELD, channelOf } from '../utils';
	import { getEnginePort } from '../../engine';

	const engine = getEnginePort();

	type NetReceiverNodeType = Node<NetReceiverNodeData, 'netReceiver'>;
	let { id, data }: NodeProps<NetReceiverNodeType> = $props();

	const flow = useSvelteFlow();

	let detectedSampleRate = $state<number | null>(null);
	let detectedFormat = $state<'pcm-f32' | 'pcm-i16' | 'opus' | null>(null);
	let detectedOpusBitrate = $state<number | null>(null);
	let detectedOpusApp = $state<'voip' | 'audio' | 'low-delay' | null>(null);

	let codecLabel = $derived.by(() => {
		if (!detectedFormat) return null;
		if (detectedFormat === 'pcm-f32') return 'PCM (f32)';
		if (detectedFormat === 'pcm-i16') return 'PCM (i16)';
		if (detectedFormat === 'opus') return 'Opus';
		return detectedFormat;
	});

	let opusModeLabel = $derived.by(() => {
		if (detectedFormat !== 'opus') return null;
		const parts: string[] = [];
		if (detectedOpusBitrate) parts.push(`${Math.round(detectedOpusBitrate / 1000)} kbps`);
		if (detectedOpusApp) parts.push(detectedOpusApp);
		return parts.length > 0 ? parts.join(' · ') : null;
	});

	let srcTooltip = $derived.by(() => {
		if (!detectedSampleRate || detectedSampleRate === engine.sampleRate) return undefined;
		return `Resampling: ${formatHz(detectedSampleRate)} → ${formatHz(engine.sampleRate)}`;
	});

	let loss = $state<number | null>(null);
	let rate = $state(0); // bytes/sec
	// Jitter buffer depth: the latency this node adds, which the receive path
	// grows on its own when the link gets bursty.
	let bufferMs = $state(0);
	let prevBytes = 0;
	let prevAt = 0;
	const lossWindow = new LossWindow();

	const POLL_MS = 1000;
	function reset(now: number) {
		loss = null;
		rate = 0;
		bufferMs = 0;
		received = 0;
		detectedSampleRate = null;
		detectedFormat = null;
		detectedOpusBitrate = null;
		detectedOpusApp = null;
		prevBytes = 0;
		prevAt = now;
		lossWindow.reset();
	}
	async function poll() {
		const s = await engine.audio.netReceiverStats(id).catch(() => null);
		const now = performance.now();
		if (!s) {
			reset(now);
			return;
		}
		loss = lossWindow.update(s.packets, s.lost);
		if (prevAt > 0 && s.bytes >= prevBytes) {
			rate = ((s.bytes - prevBytes) * 1000) / (now - prevAt);
		}
		prevBytes = s.bytes;
		prevAt = now;
		bufferMs = s.bufferMs;
		received = s.channels;
		detectedSampleRate = s.sampleRate || null;
		detectedFormat = s.format || null;
		detectedOpusBitrate = s.opusBitrate || null;
		detectedOpusApp = s.opusApp || null;
	}
	// Stats exist only while the pipeline runs.
	$effect(() => {
		if (!engine.isRunning) {
			untrack(() => reset(performance.now()));
			return;
		}
		poll();
		const interval = setInterval(poll, POLL_MS);
		return () => clearInterval(interval);
	});

	let isDestroyed = false;
	onDestroy(() => {
		isDestroyed = true;
	});

	const MAX_CHANNELS = 255;
	// Highest wire index the sender has actually delivered.
	let received = $state(0);

	$effect(() => {
		engine.audio.netReceiverListen(id, data.port).catch(() => {});
	});

	const wired = useNodeConnections({ id: untrack(() => id), handleType: 'source' });
	let wiredChannels = $derived(
		wired.current.reduce((n, c) => {
			const ch = c.sourceHandle ? channelOf(c.sourceHandle) : null;
			return ch === null ? n : Math.max(n, ch);
		}, 0)
	);

	// Slots follow the stream, falling back to the cables when nothing arrives yet.
	let channelCount = $derived(Math.max(1, Math.min(Math.max(received, wiredChannels), MAX_CHANNELS)));

	$effect(() => {
		if (isDestroyed) return;
		const next = channelCount;
		if (next !== untrack(() => data.channels)) flow.updateNodeData(id, { channels: next });
	});

	function setPort(value: string) {
		const port = Math.max(1, Math.min(65535, Math.floor(Number(value)) || 0));
		flow.updateNodeData(id, { port });
	}
</script>

<Wrapper
	label="Net Receiver"
	icon={ArrowDownload}
	accent="network"
	{srcTooltip}
	hasOutput
	channelIo
	nodeId={id}
	maxChannels={MAX_CHANNELS}
	minChannels={received}
	selfGrowing
	collapsed={data.collapsed}>
	{#snippet summary()}
		<div class="flex w-52 items-center justify-between gap-2 font-mono text-[10px] text-neutral-900 tabular-nums">
			<span>:{data.port} {codecLabel ?? 'waiting'}</span><span>{formatRate(rate)}</span>
		</div>
	{/snippet}

	<div class="nodrag nopan flex w-52 flex-col gap-2">
		<div class="flex flex-col gap-1">
			<span class={CAPTION}>Listen on UDP port</span>
			<input class={FIELD} type="number" min="1" max="65535" value={data.port} onchange={(e) => setPort(e.currentTarget.value)} />
		</div>

		<!-- Stream state, one row per fact. -->
		<div class="flex flex-col font-mono text-[10px]">
			{#snippet row(label: string, value: string)}
				<div class="flex items-baseline justify-between gap-2 border-b border-theme/5 py-1 last:border-b-0">
					<span class="text-neutral-700">{label}</span>
					<span class="truncate text-neutral-1000 tabular-nums">{value}</span>
				</div>
			{/snippet}
			{@render row('Codec', codecLabel ?? 'waiting...')}
			{#if opusModeLabel}{@render row('Mode', opusModeLabel)}{/if}
			{@render row('Stream', detectedSampleRate ? `${formatHz(detectedSampleRate)}${received > 0 ? ` · ${received} ch` : ''}` : '--')}
			<div class="flex items-center justify-between gap-2 border-b border-theme/5 py-1">
				<span class="text-neutral-700">Loss</span>
				<span class="flex items-center gap-1.5 text-neutral-1000 tabular-nums">
					<SignalBars {loss} />
					{loss == null ? '--' : formatPct(loss * 100, 1)}
				</span>
			</div>
			{@render row('Rate', formatRate(rate))}
			{@render row('Added delay', bufferMs ? `${bufferMs} ms` : '--')}
		</div>
	</div>
</Wrapper>
