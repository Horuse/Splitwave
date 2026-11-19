<script lang="ts">
	import { getEnginePort } from '../../engine';
	import { Slider, Toggle } from '@splitwave/ui';
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { ChannelBalanceNodeData } from '@splitwave/nodes';
	import Wrapper from '../node.svelte';
	import ParamGraph from '../graph/param_graph.svelte';
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
		<div class="nowheel nodrag">
			<ParamGraph x={[0, 2]} y={[-24, 24]} yTicks={[-12, 0, 12]} yFormat={(v) => (v > 0 ? `+${v}` : `${v}`)} height={72}>
				{#snippet children({ x, y })}
					<line x1={x(0)} y1={y(0)} x2={x(2)} y2={y(0)} class="stroke-neutral-500" stroke-width="0.6" />
					{#each [{ at: 0.5, db: data.leftGainDb, name: 'L', cls: 'fill-red-500' }, { at: 1.5, db: data.rightGainDb, name: 'R', cls: 'fill-blue-500' }] as ch (ch.name)}
						<rect x={x(ch.at) - 14} y={Math.min(y(0), y(ch.db))} width="28" height={Math.max(1, Math.abs(y(ch.db) - y(0)))} rx="2" class={ch.cls} />
						<text x={x(ch.at)} y={y(ch.db) + (ch.db >= 0 ? -3 : 8)} text-anchor="middle" class="fill-neutral-900 font-mono" font-size="7">
							{ch.name}
							{ch.db > 0 ? '+' : ''}{ch.db.toFixed(1)}
						</text>
					{/each}
				{/snippet}
				{#snippet badge()}
					<span>{panLabel()}</span>
				{/snippet}
			</ParamGraph>
		</div>

		<Slider
			class="nodrag nopan nowheel"
			label="Left"
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
