<script lang="ts">
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { ChannelBalanceNodeData } from '$lib/modules/pipeline/types';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import Wrapper from '../node.svelte';
	import Slider from './_slider.svelte';

	type ChannelBalanceNodeType = Node<ChannelBalanceNodeData, 'channelBalance'>;
	let { id, data }: NodeProps<ChannelBalanceNodeType> = $props();

	const flow = useSvelteFlow();
	let linked = $state(false);

	function patchData(patch: Partial<ChannelBalanceNodeData>) {
		flow.updateNodeData(id, patch);
		audioMethods.updateEffect(id, patch).catch(() => {});
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

	// Map the L/R delta to a [-1, +1] indicator position. The 48 dB span
	// matches the slider range (-24..+24 each side).
	function panNorm(): number {
		return Math.max(-1, Math.min(1, diff() / 48));
	}
</script>

<Wrapper
	label="Channel Balance"
	accent="effect"
	hasInput
	hasOutput
	bypassed={data.bypassed}
	onBypass={toggleBypass}
>
	<div class="flex w-50 flex-col gap-1.5">
		<div class="flex flex-col gap-0.5 text-[10px] text-neutral-1000">
			<div class="flex items-baseline justify-between">
				<span class="opacity-60">L</span>
				<span class="font-mono tabular-nums text-neutral-900">{panLabel()}</span>
				<span class="opacity-60">R</span>
			</div>
			<div class="relative h-2 w-full overflow-hidden rounded border border-neutral-300 bg-neutral-200">
				<div class="absolute top-0 bottom-0 left-1/2 w-px bg-neutral-500"></div>
				{#if panNorm() >= 0}
					<div
						class="absolute top-0 bottom-0 left-1/2 bg-emerald-500/60"
						style="width: {panNorm() * 50}%;"
					></div>
				{:else}
					<div
						class="absolute top-0 bottom-0 right-1/2 bg-emerald-500/60"
						style="width: {-panNorm() * 50}%;"
					></div>
				{/if}
			</div>
		</div>

		<Slider
			label="Left"
			value={data.leftGainDb}
			min={-24}
			max={24}
			step={0.1}
			unit=" dB"
			defaultValue={0}
			ticks={[-12, -6, 0, 6, 12]}
			onChange={setLeft}
		/>
		<Slider
			label="Right"
			value={data.rightGainDb}
			min={-24}
			max={24}
			step={0.1}
			unit=" dB"
			defaultValue={0}
			ticks={[-12, -6, 0, 6, 12]}
			onChange={setRight}
		/>

		<div class="flex items-center justify-between text-[10px] text-neutral-1000">
			<button
				type="button"
				class="nodrag nopan rounded border border-neutral-300 bg-neutral-100 px-2 py-0.5 hover:bg-neutral-200"
				onclick={center}
			>
				Center
			</button>
			<label class="nodrag nopan flex items-center gap-1">
				<input type="checkbox" bind:checked={linked} />
				Link inverse
			</label>
		</div>
	</div>
</Wrapper>
