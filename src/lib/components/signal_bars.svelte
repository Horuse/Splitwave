<script lang="ts">
	// Cellular-style signal strength. Quality is derived from packet loss and/or
	// ping; whichever metrics are provided are combined by taking the worst.
	// Colours: green (good) / yellow (fair) / red (poor), grey when unknown.
	interface Props {
		loss?: number | null;
		ping?: number | null;
		bars?: number;
	}
	let { loss = null, ping = null, bars = 4 }: Props = $props();

	// Each metric maps to a 0..4 level; the shown level is the worst available.
	function lossLevel(l: number): number {
		if (l < 0.01) return 4;
		if (l < 0.03) return 3;
		if (l < 0.08) return 2;
		if (l < 0.2) return 1;
		return 0;
	}
	function pingLevel(p: number): number {
		if (p < 60) return 4;
		if (p < 120) return 3;
		if (p < 200) return 2;
		if (p < 350) return 1;
		return 0;
	}

	let level = $derived.by(() => {
		const levels: number[] = [];
		if (loss != null) levels.push(lossLevel(loss));
		if (ping != null && ping > 0) levels.push(pingLevel(ping));
		if (levels.length === 0) return -1; // unknown
		return Math.min(...levels);
	});

	// 4-level scale mapped onto the rendered bar count.
	let filled = $derived(level < 0 ? 0 : Math.round((level / 4) * bars));
	let color = $derived(
		level < 0
			? 'unknown'
			: level >= 3
				? 'good'
				: level === 2
					? 'fair'
					: 'poor'
	);
</script>

<div class="flex items-end gap-[2px]" title={level < 0 ? 'no data' : `quality ${level}/4`}>
	{#each Array(bars) as _, i (i)}
		<span
			class="w-[3px] rounded-[1px]"
			class:bg-neutral-300={i >= filled || color === 'unknown'}
			class:bg-green-500={i < filled && color === 'good'}
			class:bg-yellow-500={i < filled && color === 'fair'}
			class:bg-red-500={i < filled && color === 'poor'}
			style="height: {4 + i * 3}px"
		></span>
	{/each}
</div>
