<script lang="ts">
	import { onDestroy } from 'svelte';
	import { useSvelteFlow, Handle, Position, type Node, type NodeProps } from '@xyflow/svelte';
	import type { NetReceiverNodeData } from '$lib/modules/pipeline/types';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import SignalBars from '$lib/components/signal_bars.svelte';
	import { formatRate, LossWindow } from '$lib/components/format';
	import Wrapper from '../node.svelte';

	type NetReceiverNodeType = Node<NetReceiverNodeData, 'netReceiver'>;
	let { id, data }: NodeProps<NetReceiverNodeType> = $props();

	const flow = useSvelteFlow();

	let loss = $state<number | null>(null);
	let rate = $state(0); // bytes/sec
	let prevBytes = 0;
	let prevAt = 0;
	const lossWindow = new LossWindow();

	const POLL_MS = 1000;
	const interval = setInterval(async () => {
		const s = await audioMethods.netReceiverStats(id).catch(() => null);
		const now = performance.now();
		if (!s) {
			loss = null;
			rate = 0;
			prevBytes = 0;
			prevAt = now;
			lossWindow.reset();
			return;
		}
		loss = lossWindow.update(s.packets, s.lost);
		if (prevAt > 0 && s.bytes >= prevBytes) {
			rate = ((s.bytes - prevBytes) * 1000) / (now - prevAt);
		}
		prevBytes = s.bytes;
		prevAt = now;
	}, POLL_MS);
	onDestroy(() => clearInterval(interval));

	const MAX_CHANNELS = 10;
	let channelCount = $derived(Math.min(Math.max(data.channels ?? 1, 1), MAX_CHANNELS));

	function setPort(value: string) {
		const port = Math.max(1, Math.min(65535, Math.floor(Number(value)) || 0));
		flow.updateNodeData(id, { port });
	}

	function addChannel() {
		if (channelCount < MAX_CHANNELS) flow.updateNodeData(id, { channels: channelCount + 1 });
	}

	function removeChannel() {
		if (channelCount <= 1) return;
		const handle = String(channelCount - 1);
		const orphaned = flow
			.getEdges()
			.filter((e) => e.source === id && e.sourceHandle === handle)
			.map((e) => ({ id: e.id }));
		if (orphaned.length > 0) flow.deleteElements({ edges: orphaned });
		flow.updateNodeData(id, { channels: channelCount - 1 });
	}
</script>

<Wrapper label="Net Receiver" accent="input">
	<div class="nodrag nopan flex w-44 flex-col gap-2">
		<!-- port -->
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

		<!-- quality + throughput -->
		<div class="flex items-center justify-between">
			<div class="flex items-center gap-1">
				<SignalBars {loss} />
				<span class="font-mono text-[9px] tabular-nums text-neutral-500">
					{loss == null ? '--' : `${(loss * 100).toFixed(1)}%`}
				</span>
			</div>
			<span class="font-mono text-[9px] tabular-nums text-neutral-500">{formatRate(rate)}</span>
		</div>

		<hr class="border-neutral-300" />

		<!-- mix output (default handle) -->
		<div class="relative -mr-4 flex min-h-5 items-center justify-between gap-1 pr-4">
			<span class="truncate font-mono text-[9px] text-neutral-500">all inputs</span>
			<span class="shrink-0 font-mono text-[9px] text-neutral-400">mix</span>
			<Handle type="source" position={Position.Right} class="handle" />
		</div>
		<!-- per-input outputs -->
		{#each Array(channelCount) as _, c (c)}
			<div class="relative -mr-4 flex min-h-5 items-center justify-end gap-1 pr-4">
				<span class="shrink-0 font-mono text-[9px] text-neutral-400">in {c + 1}</span>
				<Handle type="source" id={String(c)} position={Position.Right} class="handle" />
			</div>
		{/each}
	</div>
</Wrapper>
