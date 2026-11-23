<script lang="ts">
	import { getEnginePort } from '../../engine';
	import { Slider, Toggle } from '@splitwave/ui';
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { ChannelBalanceNodeData } from '@splitwave/nodes';
	import Wrapper from '../node.svelte';
	import { Balance } from '../icons';

	type ChannelBalanceNodeType = Node<ChannelBalanceNodeData, 'channelBalance'>;
	let { id, data }: NodeProps<ChannelBalanceNodeType> = $props();

	const engine = getEnginePort();

	const flow = useSvelteFlow();
	let linked = $state(false);

	function patchData(patch: Partial<ChannelBalanceNodeData>) {
		flow.updateNodeData(id, patch);
		engine.updateEffect(id, patch).catch(() => {});
	}

	function toggleBypass() {
		patchData({ bypassed: !data.bypassed });
	}

	function setLeft(v: number) {
		const patch: Partial<ChannelBalanceNodeData> = { leftGainDb: v };
		if (linked) patch.rightGainDb = -v;
		patchData(patch);
	}
	function setRight(v: number) {
		const patch: Partial<ChannelBalanceNodeData> = { rightGainDb: v };
		if (linked) patch.leftGainDb = -v;
		patchData(patch);
	}
	function center() {
		patchData({ leftGainDb: 0, rightGainDb: 0 });
	}

	function diff(): number {
		return data.rightGainDb - data.leftGainDb;
	}

	function panLabel(): string {
		const d = diff();
		if (Math.abs(d) < 0.05) return 'Center';
		return d > 0 ? `${d.toFixed(1)} dB R` : `${(-d).toFixed(1)} dB L`;
	}
</script>

<Wrapper label="Channel Balance" icon={Balance} accent="effect" hasInput hasOutput channelIo nodeId={id} bypassed={data.bypassed} onBypass={toggleBypass}>
	<div class="flex w-50 flex-col gap-1.5">
		<div class="flex items-baseline justify-between text-[10px] text-neutral-800">
			<span>L</span>
			<span class="font-mono tabular-nums">{panLabel()}</span>
			<span>R</span>
		</div>

		<Slider
			class="nodrag nopan nowheel"
			label="Left"
			info="Gain of the left channel in dB."
			value={data.leftGainDb}
			min={-24}
			max={24}
			step={0.1}
			unit=" dB"
			defaultValue={0}
			ticks={[-12, -6, 0, 6, 12]}
			onChange={setLeft} />
		<Slider
			class="nodrag nopan nowheel"
			label="Right"
			info="Gain of the right channel in dB. With Link inverse on, moving one side moves the other the opposite way."
			value={data.rightGainDb}
			min={-24}
			max={24}
			step={0.1}
			unit=" dB"
			defaultValue={0}
			ticks={[-12, -6, 0, 6, 12]}
			onChange={setRight} />

		<div class="flex items-center justify-between text-[10px] text-neutral-1000">
			<button type="button" class="nodrag nopan rounded-md bg-(--lv-1) px-2 py-0.5 hover:bg-(--lv-2)" onclick={center}> Center </button>
			<Toggle class="nodrag nopan" size="sm" label="Link inverse" checked={linked} onChange={(v) => (linked = v)} />
		</div>
	</div>
</Wrapper>
