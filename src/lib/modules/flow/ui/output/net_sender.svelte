<script lang="ts">
	import { onDestroy } from 'svelte';
	import { useSvelteFlow, Handle, Position, type Node, type NodeProps } from '@xyflow/svelte';
	import type { NetSenderNodeData, NetCodec, OpusApplication } from '$lib/modules/pipeline/types';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import { formatRate } from '$lib/components/format';
	import Wrapper from '../node.svelte';

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

	const MAX_CHANNELS = 10;
	let channelCount = $derived(Math.min(Math.max(data.channels ?? 1, 1), MAX_CHANNELS));
	let inputs = $derived(
		Array.from({ length: channelCount }, (_, i) => ({ id: `ch${i + 1}`, label: `in ${i + 1}` }))
	);

	function setTargetIp(value: string) {
		flow.updateNodeData(id, { targetIp: value.trim() });
	}

	function setPort(value: string) {
		const port = Math.max(1, Math.min(65535, Math.floor(Number(value)) || 0));
		flow.updateNodeData(id, { port });
	}

	function addChannel() {
		if (channelCount < MAX_CHANNELS) flow.updateNodeData(id, { channels: channelCount + 1 });
	}

	function removeChannel() {
		if (channelCount <= 1) return;
		const handle = `ch${channelCount}`;
		const orphaned = flow
			.getEdges()
			.filter((e) => e.target === id && e.targetHandle === handle)
			.map((e) => ({ id: e.id }));
		if (orphaned.length > 0) flow.deleteElements({ edges: orphaned });
		flow.updateNodeData(id, { channels: channelCount - 1 });
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

<Wrapper label="Net Sender" accent="output">
	<div class="nodrag nopan flex w-48 flex-col gap-2">
		<!-- target -->
		<div class="flex flex-col gap-0.5">
			<span class="font-mono text-[9px] text-neutral-500">Target IP</span>
			<input
				class="nowheel h-6 rounded border border-neutral-300 bg-neutral-50 px-1.5 font-mono text-[10px] text-neutral-800 placeholder:text-neutral-400"
				placeholder="192.168.1.20"
				value={data.targetIp ?? ''}
				onchange={(e) => setTargetIp(e.currentTarget.value)}
			/>
		</div>
		<div class="flex flex-col gap-0.5">
			<span class="font-mono text-[9px] text-neutral-500">UDP port</span>
			<input
				class="nowheel h-6 rounded border border-neutral-300 bg-neutral-50 px-1.5 font-mono text-[10px] text-neutral-800"
				type="number"
				min="1"
				max="65535"
				value={data.port}
				onchange={(e) => setPort(e.currentTarget.value)}
			/>
		</div>

		<!-- inputs -->
		<div class="flex items-center justify-between">
			<span class="font-mono text-[9px] text-neutral-500">Inputs</span>
			<div class="flex items-center gap-1">
				<button
					type="button"
					class="nodrag nopan button-main secondary flex h-4 w-4 items-center justify-center rounded p-0 font-mono text-[11px] leading-none"
					disabled={channelCount <= 1}
					onclick={removeChannel}
				>
					-
				</button>
				<span class="w-4 text-center font-mono text-[10px] tabular-nums text-neutral-900">{channelCount}</span>
				<button
					type="button"
					class="nodrag nopan button-main secondary flex h-4 w-4 items-center justify-center rounded p-0 font-mono text-[11px] leading-none"
					disabled={channelCount >= MAX_CHANNELS}
					onclick={addChannel}
				>
					+
				</button>
			</div>
		</div>

		<!-- per-channel inputs (left handles) -->
		{#each inputs as ch (ch.id)}
			<div class="relative -ml-4 flex min-h-3 items-center gap-1 pl-4">
				<Handle type="target" id={ch.id} position={Position.Left} class="handle" />
				<span class="font-mono text-[9px] text-neutral-500">{ch.label}</span>
			</div>
		{/each}

		<!-- throughput -->
		<div class="flex items-center justify-between">
			<span class="font-mono text-[9px] text-neutral-500">Sending</span>
			<span class="font-mono text-[9px] tabular-nums text-neutral-500">{formatRate(rate)}</span>
		</div>

		<hr class="border-neutral-300" />

		<!-- codec -->
		<div class="flex flex-col gap-0.5">
			<span class="font-mono text-[9px] text-neutral-500">Codec</span>
			<div class="grid grid-cols-3 gap-[2px] rounded-sm border border-neutral-300 p-[2px]">
				{#each CODECS as c (c.value)}
					<button
						type="button"
						onclick={() => setCodec(c.value)}
						class={[
							'flex flex-col items-center rounded-sm py-0.5 leading-none transition-colors',
							data.codec === c.value
								? 'bg-neutral-900 text-white'
								: 'bg-neutral-100 text-neutral-900 hover:bg-neutral-200'
						]}
					>
						<span class="font-mono text-[10px]">{c.label}</span>
						<span class="text-[8px] opacity-70">{c.sub}</span>
					</button>
				{/each}
			</div>
		</div>

		{#if data.codec === 'opus'}
			<!-- bitrate -->
			<div class="flex flex-col gap-0.5">
				<span class="font-mono text-[9px] text-neutral-500">Bitrate (kbps)</span>
				<div class="grid grid-cols-4 gap-[2px] rounded-sm border border-neutral-300 p-[2px]">
					{#each BITRATES as b (b.bps)}
						<button
							type="button"
							onclick={() => setBitrate(b.bps)}
							class={[
								'rounded-sm py-0.5 font-mono text-[10px] leading-none transition-colors',
								data.opusBitrate === b.bps
									? 'bg-neutral-900 text-white'
									: 'bg-neutral-100 text-neutral-900 hover:bg-neutral-200'
							]}
						>
							{b.label}
						</button>
					{/each}
				</div>
			</div>

			<!-- application -->
			<div class="flex flex-col gap-0.5">
				<span class="font-mono text-[9px] text-neutral-500">Mode</span>
				<div class="grid grid-cols-3 gap-[2px] rounded-sm border border-neutral-300 p-[2px]">
					{#each APPS as a (a.value)}
						<button
							type="button"
							onclick={() => setApp(a.value)}
							class={[
								'flex flex-col items-center rounded-sm py-0.5 leading-none transition-colors',
								data.opusApplication === a.value
									? 'bg-neutral-900 text-white'
									: 'bg-neutral-100 text-neutral-900 hover:bg-neutral-200'
							]}
						>
							<span class="font-mono text-[10px]">{a.label}</span>
							<span class="text-[8px] opacity-70">{a.sub}</span>
						</button>
					{/each}
				</div>
			</div>
		{/if}
	</div>
</Wrapper>
