<script lang="ts">
	import { onDestroy, untrack } from 'svelte';
	import { useNodeConnections, useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { NetSenderNodeData, NetCodec, OpusApplication } from '$lib/modules/pipeline/types';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import { formatRate } from '$lib/components/format';
	import { parseHandle } from '$lib/modules/flow/utils';
	import Wrapper from '../node.svelte';
	import { ArrowUpload } from '$lib/components/icons';
	import SegmentedButtons from '$lib/components/segmented_buttons.svelte';

	type NetSenderNodeType = Node<NetSenderNodeData, 'netSender'>;
	let { id, data }: NodeProps<NetSenderNodeType> = $props();

	const flow = useSvelteFlow();

	let rate = $state(0); // bytes/sec
	let prevBytes = 0;
	let prevAt = 0;

	const interval = setInterval(async () => {
		const s = await audioMethods.netSenderStats(id).catch(() => null);
		const now = performance.now();
		if (!s) {
			rate = 0;
			prevBytes = 0;
			prevAt = now;
			return;
		}
		if (prevAt > 0 && s.bytes >= prevBytes) {
			rate = ((s.bytes - prevBytes) * 1000) / (now - prevAt);
		}
		prevBytes = s.bytes;
		prevAt = now;
	}, 1000);
	onDestroy(() => clearInterval(interval));

	const MAX_CHANNELS = 255;
	const wired = useNodeConnections({ id: untrack(() => id), handleType: 'target' });
	let wiredChannels = $derived(
		wired.current.reduce((n, c) => {
			const ch = c.targetHandle ? parseHandle(c.targetHandle) : null;
			return ch === null ? n : Math.max(n, ch);
		}, 0)
	);

	// The sender opens one send ring per channel, so it tracks the cables.
	$effect(() => {
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
</script>

<Wrapper label="Net Sender" icon={ArrowUpload} accent="network" hasInput channelIo nodeId={id} maxChannels={MAX_CHANNELS}>
	<div class="nodrag nopan flex w-48 flex-col gap-2">
		<!-- target -->
		<div class="flex flex-col gap-0.5">
			<span class="font-mono text-[9px] text-neutral-500">Target IP</span>
			<input
				class="nowheel h-6 rounded border border-neutral-300 bg-neutral-50 px-1.5 font-mono text-[10px] text-neutral-800 placeholder:text-neutral-400"
				placeholder="192.168.1.20"
				value={data.targetIp ?? ''}
				onchange={(e) => setTargetIp(e.currentTarget.value)} />
		</div>
		<div class="flex flex-col gap-0.5">
			<span class="font-mono text-[9px] text-neutral-500">UDP port</span>
			<input
				class="nowheel h-6 rounded border border-neutral-300 bg-neutral-50 px-1.5 font-mono text-[10px] text-neutral-800"
				type="number"
				min="1"
				max="65535"
				value={data.port}
				onchange={(e) => setPort(e.currentTarget.value)} />
		</div>

		<!-- throughput -->
		<div class="flex items-center justify-between">
			<span class="font-mono text-[9px] text-neutral-500">Sending</span>
			<span class="font-mono text-[9px] text-neutral-500 tabular-nums">{formatRate(rate)}</span>
		</div>

		<hr class="border-neutral-300" />

		<!-- codec -->
		<div class="flex flex-col gap-0.5">
			<span class="font-mono text-[9px] text-neutral-500">Codec</span>
			<SegmentedButtons options={CODECS.map((c) => ({ value: c.value, label: c.label, subtitle: c.sub }))} value={data.codec} onSelect={setCodec} />
		</div>

		{#if data.codec === 'opus'}
			<!-- bitrate -->
			<div class="flex flex-col gap-0.5">
				<span class="font-mono text-[9px] text-neutral-500">Bitrate (kbps)</span>
				<SegmentedButtons options={BITRATES.map((b) => ({ value: b.bps, label: b.label }))} value={data.opusBitrate} onSelect={setBitrate} />
			</div>

			<!-- application -->
			<div class="flex flex-col gap-0.5">
				<span class="font-mono text-[9px] text-neutral-500">Mode</span>
				<SegmentedButtons
					options={APPS.map((a) => ({ value: a.value, label: a.label, subtitle: a.sub }))}
					value={data.opusApplication}
					onSelect={setApp} />
			</div>
		{/if}
	</div>
</Wrapper>
