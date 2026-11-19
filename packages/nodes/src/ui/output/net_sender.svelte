<script lang="ts">
	import { NumberStepper, Segmented } from '@splitwave/ui';
	import { onDestroy, untrack } from 'svelte';
	import { useNodeConnections, useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { NetSenderNodeData, NetCodec, OpusApplication } from '@splitwave/nodes';
	import { formatHz, formatRate, formatKhzValue } from '@splitwave/nodes';
	import { CAPTION, FIELD, channelOf } from '../utils';
	import Wrapper from '../node.svelte';
	import ArrowUpload from '~icons/fluent/arrow-upload-16-filled';
	import { getEnginePort } from '../../engine';

	const engine = getEnginePort();

	type NetSenderNodeType = Node<NetSenderNodeData, 'netSender'>;
	let { id, data }: NodeProps<NetSenderNodeType> = $props();

	const flow = useSvelteFlow();

	let srcTooltip = $derived.by(() => {
		if (data.codec === 'opus') {
			if (engine.sampleRate === 48_000) return undefined;
			return `Resampling: ${formatHz(engine.sampleRate)} → ${formatHz(48_000)}`;
		}
		const targetSr = data.sampleRate ?? engine.sampleRate;
		if (targetSr === engine.sampleRate) return undefined;
		return `Resampling: ${formatHz(engine.sampleRate)} → ${formatHz(targetSr)}`;
	});

	let rate = $state(0); // bytes/sec
	let prevBytes = 0;
	let prevAt = 0;

	function reset(now: number) {
		rate = 0;
		prevBytes = 0;
		prevAt = now;
	}
	async function poll() {
		const s = await engine.audio.netSenderStats(id).catch(() => null);
		const now = performance.now();
		if (!s) {
			reset(now);
			return;
		}
		if (prevAt > 0 && s.bytes >= prevBytes) {
			rate = ((s.bytes - prevBytes) * 1000) / (now - prevAt);
		}
		prevBytes = s.bytes;
		prevAt = now;
	}
	// Stats exist only while the pipeline runs.
	$effect(() => {
		if (!engine.isRunning) {
			untrack(() => reset(performance.now()));
			return;
		}
		poll();
		const interval = setInterval(poll, 1000);
		return () => clearInterval(interval);
	});

	let isDestroyed = false;
	onDestroy(() => {
		isDestroyed = true;
	});

	const MAX_CHANNELS = 255;
	const wired = useNodeConnections({ id: untrack(() => id), handleType: 'target' });
	let wiredChannels = $derived(
		wired.current.reduce((n, c) => {
			const ch = c.targetHandle ? channelOf(c.targetHandle) : null;
			return ch === null ? n : Math.max(n, ch);
		}, 0)
	);

	// The sender opens one send ring per channel, so it tracks the cables.
	$effect(() => {
		if (isDestroyed) return;
		const next = Math.max(1, Math.min(wiredChannels, MAX_CHANNELS));
		if (next !== untrack(() => data.channels)) flow.updateNodeData(id, { channels: next });
	});

	function setTargetIp(value: string) {
		flow.updateNodeData(id, { targetIp: value.trim() });
	}

	function setPort(value: string) {
		const port = Math.max(1, Math.min(65535, Math.floor(Number(value)) || 0));
		flow.updateNodeData(id, { port });
	}

	const CODECS: { value: NetCodec; label: string; sub: string }[] = [
		{ value: 'opus', label: 'Opus', sub: 'compressed' },
		{ value: 'pcm-f32', label: 'PCM', sub: 'f32' },
		{ value: 'pcm-i16', label: 'PCM', sub: 'i16' }
	];

	const BITRATES: { bps: number; label: string }[] = [
		{ bps: 32_000, label: '32' },
		{ bps: 64_000, label: '64' },
		{ bps: 96_000, label: '96' },
		{ bps: 128_000, label: '128' }
	];

	const APPS: { value: OpusApplication; label: string; sub: string }[] = [
		{ value: 'voip', label: 'VoIP', sub: 'voice' },
		{ value: 'audio', label: 'Audio', sub: 'music' },
		{ value: 'low-delay', label: 'Low', sub: 'delay' }
	];

	function setCodec(codec: NetCodec) {
		flow.updateNodeData(id, { codec });
	}

	function setBitrate(bps: number) {
		flow.updateNodeData(id, { opusBitrate: bps });
	}

	function setApp(app: OpusApplication) {
		flow.updateNodeData(id, { opusApplication: app });
	}

	const PRESET_RATES = [44_100, 48_000, 88_200, 96_000];
	const rateValues = new Set(PRESET_RATES.map(String));

	let customRateSelected = $state(false);

	let rateSelection = $derived.by(() => {
		if (data.sampleRate === null) return 'auto';
		if (customRateSelected || !rateValues.has(String(data.sampleRate))) return 'custom';
		return String(data.sampleRate);
	});

	let rateOptions = $derived([
		{ value: 'auto', label: 'Auto' },
		...PRESET_RATES.map((r) => ({ value: String(r), label: formatKhzValue(r) })),
		{ value: 'custom', label: 'Custom' }
	]);

	function setRateSelection(sel: string) {
		if (sel === 'custom') {
			customRateSelected = true;
			if (data.sampleRate === null) {
				flow.updateNodeData(id, { sampleRate: engine.sampleRate });
			}
			return;
		}
		customRateSelected = false;
		if (sel === 'auto') {
			flow.updateNodeData(id, { sampleRate: null });
		} else {
			flow.updateNodeData(id, { sampleRate: Number(sel) });
		}
	}

	function setCustomRate(n: number) {
		const next = Math.min(384_000, Math.max(8_000, Math.round(n)));
		flow.updateNodeData(id, { sampleRate: next });
	}
</script>

<Wrapper label="Net Sender" icon={ArrowUpload} accent="network" {srcTooltip} hasInput channelIo nodeId={id} maxChannels={MAX_CHANNELS}>
	<div class="nodrag nopan flex w-52 flex-col gap-2">
		<!-- Target: address and UDP port on one line. -->
		<div class="flex flex-col gap-1">
			<span class={CAPTION}>Target</span>
			<div class="grid grid-cols-[1fr_4.5rem] gap-1">
				<input class={FIELD} placeholder="192.168.1.20" value={data.targetIp ?? ''} onchange={(e) => setTargetIp(e.currentTarget.value)} />
				<input class={FIELD} type="number" min="1" max="65535" title="UDP port" value={data.port} onchange={(e) => setPort(e.currentTarget.value)} />
			</div>
		</div>

		<div class="flex items-center justify-between font-mono text-[10px]">
			<span class="flex items-center gap-1.5 text-neutral-700">
				<span class={['size-1.5 rounded-full', rate > 0 ? 'bg-emerald-500' : 'bg-neutral-500']}></span>
				{rate > 0 ? 'Sending' : 'Idle'}
			</span>
			<span class="text-neutral-1000 tabular-nums">{formatRate(rate)}</span>
		</div>

		<div class="border-t border-theme/5"></div>

		<!-- codec -->
		<div class="flex flex-col gap-0.5">
			<span class={CAPTION}>Codec</span>
			<Segmented
				class="nodrag nopan"
				options={CODECS.map((c) => ({ value: c.value, label: c.label, subtitle: c.sub }))}
				value={data.codec}
				onSelect={setCodec} />
		</div>

		{#if data.codec === 'opus'}
			<!-- bitrate -->
			<div class="flex flex-col gap-0.5">
				<span class={CAPTION}>Bitrate (kbps)</span>
				<Segmented
					class="nodrag nopan"
					options={BITRATES.map((b) => ({ value: b.bps, label: b.label }))}
					value={data.opusBitrate}
					onSelect={setBitrate} />
			</div>

			<!-- application -->
			<div class="flex flex-col gap-0.5">
				<span class={CAPTION}>Mode</span>
				<Segmented
					class="nodrag nopan"
					options={APPS.map((a) => ({ value: a.value, label: a.label, subtitle: a.sub }))}
					value={data.opusApplication}
					onSelect={setApp} />
			</div>
		{:else}
			<!-- sample rate -->
			<div class="flex flex-col gap-1">
				<Segmented
					class="nodrag nopan"
					label="Sample rate"
					note="kHz"
					options={rateOptions}
					value={rateSelection}
					onSelect={setRateSelection}
					columns={3} />
				{#if rateSelection === 'custom'}
					<div class="flex items-center justify-end gap-1">
						<span class={CAPTION}>Hz</span>
						<NumberStepper
							class="nodrag nopan"
							value={data.sampleRate ?? engine.sampleRate}
							min={8000}
							max={384000}
							step={100}
							label="Sample rate"
							onchange={setCustomRate} />
					</div>
				{/if}
			</div>
		{/if}
	</div>
</Wrapper>
