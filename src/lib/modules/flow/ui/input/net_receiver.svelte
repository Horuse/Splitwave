<script lang="ts">
	import { onDestroy, untrack } from 'svelte';
	import {
		useNodeConnections,
		useSvelteFlow,
		Handle,
		Position,
		type Node,
		type NodeProps
	} from '@xyflow/svelte';
	import type { NetReceiverNodeData } from '$lib/modules/pipeline/types';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import SignalBars from '$lib/components/signal_bars.svelte';
	import { formatRate, LossWindow } from '$lib/components/format';
	import Wrapper from '../node.svelte';
	import { parseHandle } from '$lib/modules/flow/utils';

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
			received = 0;
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
		received = s.channels;
	}, POLL_MS);
	onDestroy(() => clearInterval(interval));

	const MAX_CHANNELS = 255;
	// Highest wire index the sender has actually delivered.
	let received = $state(0);

	$effect(() => {
		audioMethods.netReceiverListen(id, data.port).catch(() => {});
	});

	onDestroy(() => audioMethods.netReceiverRelease(id).catch(() => {}));
	const wired = useNodeConnections({ id: untrack(() => id), handleType: 'source' });
	let wiredChannels = $derived(
		wired.current.reduce((n, c) => {
			const ch = c.sourceHandle ? parseHandle(c.sourceHandle) : null;
			return ch === null ? n : Math.max(n, ch);
		}, 0)
	);

	// Slots follow the stream, falling back to the cables when nothing arrives yet.
	let channelCount = $derived(
		Math.max(1, Math.min(Math.max(received, wiredChannels), MAX_CHANNELS))
	);

	$effect(() => {
		const next = channelCount;
		if (next !== untrack(() => data.channels)) flow.updateNodeData(id, { channels: next });
	});

	function setPort(value: string) {
		const port = Math.max(1, Math.min(65535, Math.floor(Number(value)) || 0));
		flow.updateNodeData(id, { port });
	}

</script>

<Wrapper label="Net Receiver" accent="input" hasOutput channelIo nodeId={id} maxChannels={MAX_CHANNELS} minChannels={received} selfGrowing>
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

		<div class="relative -mr-4 flex min-h-5 items-center justify-between gap-1 pr-4">
			<span class="truncate font-mono text-[9px] text-neutral-500">all inputs</span>
			<span class="shrink-0 font-mono text-[9px] text-neutral-400">mix</span>
			<Handle type="source" position={Position.Right} class="handle" />
		</div>
	</div>
</Wrapper>
