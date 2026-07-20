<script lang="ts">
	import { listen, type UnlistenFn } from '@tauri-apps/api/event';
	import { onDestroy, onMount } from 'svelte';
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { LufsMeterNodeData } from '$lib/modules/pipeline/types';
	import Wrapper from '../node.svelte';
	import { Gauge, Refresh } from '$lib/components/icons';
	import { onNodeAction } from '$lib/modules/flow/utils';
	import MeterBar from '$lib/components/meter_bar.svelte';
	import { Combobox } from '$lib/modules/form/ui';

	type LufsMeterNodeType = Node<LufsMeterNodeData, 'lufsMeter'>;
	let { id, data }: NodeProps<LufsMeterNodeType> = $props();

	const flow = useSvelteFlow();

	// Sentinel emitted from the engine when LUFS is `-inf` (silent).
	const LUFS_SILENT = -120;

	const LUFS_FLOOR = -40; // bar scale floor

	const BAR_W = 42; // px per channel bar — wide enough for "-15.2"
	const BAR_H = 235; // bars and their scale rail must stay the same height

	const RMS_FLOOR = -80; // noise floor sits near -60, so the LUFS scale is too short

	interface Profile {
		id: string;
		label: string;
		note: string;
		target?: number;
		/** ACX is the one profile specified as an RMS window, not a single target. */
		rmsRange?: [number, number];
		truePeakMax?: number;
		noiseFloorMax?: number;
	}

	const PROFILES: Profile[] = [
		{ id: 'free', label: 'Free', note: 'No target' },
		{
			id: 'ebu',
			label: 'EBU R128',
			note: '-23 LUFS, -1 dBTP',
			target: -23,
			truePeakMax: -1
		},
		{
			id: 'bs1770',
			label: 'ITU-R BS.1770',
			note: '-23 LUFS, -1 dBTP',
			target: -23,
			truePeakMax: -1
		},
		{
			id: 'atsc',
			label: 'ATSC A/85',
			note: '-24 LKFS, -2 dBTP',
			target: -24,
			truePeakMax: -2
		},
		{
			id: 'aes',
			label: 'AES TD1008',
			note: '-16 LUFS, -1 dBTP',
			target: -16,
			truePeakMax: -1
		},
		{
			id: 'apple',
			label: 'Apple',
			note: '-16 LUFS, -1 dBTP',
			target: -16,
			truePeakMax: -1
		},
		{
			id: 'spotify',
			label: 'Spotify',
			note: '-14 LUFS, -1 dBTP',
			target: -14,
			truePeakMax: -1
		},
		{
			id: 'acx',
			label: 'ACX',
			note: 'RMS -23..-18, peak -3, floor -60',
			rmsRange: [-23, -18],
			truePeakMax: -3,
			noiseFloorMax: -60
		}
	];

	const LUFS_GRADIENT = `linear-gradient(to top,
        #22c55e 0%, #22c55e 55%,
        #eab308 55%, #eab308 80%,
        #f97316 80%, #f97316 92%,
        #ef4444 92%, #ef4444 100%)`;

	const LUFS_TICKS = [0, -6, -12, -18, -23, -30, -40];
	const RMS_TICKS = [0, -12, -18, -23, -40, -60, -80];

	let profile = $derived(PROFILES.find((p) => p.id === (data.profile ?? 'free')) ?? PROFILES[0]);

	// `target` stays in the data so the older LUFS-only readouts keep working.
	function setProfile(profileId: string | null) {
		const next = PROFILES.find((p) => p.id === profileId) ?? PROFILES[0];
		flow.updateNodeData(id, { profile: next.id, target: next.target ?? null });
	}

	function dbToPct(db: number, floor: number): number {
		if (!isFinite(db) || db <= LUFS_SILENT) return 0;
		return Math.max(0, Math.min(100, ((db - floor) / -floor) * 100));
	}

	function hoverLabel(floor: number): (pct: number) => string {
		return (pct) => (floor + (pct / 100) * -floor).toFixed(1);
	}

	let momentary = $state(LUFS_SILENT);
	let shortterm = $state(LUFS_SILENT);
	let integrated = $state(LUFS_SILENT);
	let holdM = $state(LUFS_SILENT);
	let holdS = $state(LUFS_SILENT);
	let holdI = $state(LUFS_SILENT);
	let tpMax = $state(LUFS_SILENT);
	let lra = $state(0);
	let rms = $state(LUFS_SILENT);
	let noiseFloor = $state(LUFS_SILENT);
	let samplePeak = $state(LUFS_SILENT);
	let dcOffset = $state(0);
	let correlation = $state(1);
	let clips = $state(0);

	interface LufsTick {
		nodeId: string;
		momentary: number;
		shortterm: number;
		integrated: number;
		tpL: number;
		tpR: number;
		lra: number;
		rms: number;
		noiseFloor: number;
		samplePeak: number;
		dcOffset: number;
		correlation: number;
		clips: number;
	}

	/** DC is a linear mean, so it needs its own conversion rather than `format`. */
	function formatDc(v: number): string {
		const a = Math.abs(v);
		return a < 1e-6 ? '−∞' : (20 * Math.log10(a)).toFixed(1);
	}

	function format(v: number): string {
		return v <= LUFS_SILENT ? '−∞' : v.toFixed(1);
	}

	// Peak-to-loudness style ratios; both operands must be valid, else no reading.
	function ratio(peak: number, loud: number): string {
		if (peak <= LUFS_SILENT || loud <= LUFS_SILENT) return '—';
		return (peak - loud).toFixed(1);
	}

	function targetDelta(integrated: number, target: number | null | undefined): string | null {
		if (integrated <= LUFS_SILENT || target == null) return null;
		const d = integrated - target;
		return `${d >= 0 ? '+' : ''}${d.toFixed(1)} LU`;
	}

	function targetClass(integrated: number, target: number | null | undefined): string {
		if (integrated <= LUFS_SILENT) return 'text-neutral-400';
		if (target == null) return 'text-neutral-900';
		const delta = Math.abs(integrated - target);
		if (delta <= 0.5) return 'text-green-600';
		if (delta <= 1.5) return 'text-amber-500';
		return 'text-red-500';
	}

	// Only the checks the active profile actually specifies; a LUFS profile has
	// nothing to say about a noise floor.
	let checks = $derived.by(() => {
		const out: { label: string; ok: boolean; want: string }[] = [];
		if (profile.rmsRange) {
			const [lo, hi] = profile.rmsRange;
			out.push({ label: 'RMS', ok: rms > lo && rms < hi, want: `${lo}..${hi}` });
		}
		if (profile.target != null) {
			out.push({
				label: 'Integrated',
				ok: integrated > LUFS_SILENT && Math.abs(integrated - profile.target) <= 1,
				want: `${profile.target} ±1`
			});
		}
		if (profile.truePeakMax != null) {
			out.push({
				label: 'True Peak',
				ok: tpMax > LUFS_SILENT && tpMax <= profile.truePeakMax,
				want: `≤ ${profile.truePeakMax}`
			});
		}
		if (profile.noiseFloorMax != null) {
			out.push({
				label: 'Noise Floor',
				ok: noiseFloor > LUFS_SILENT && noiseFloor <= profile.noiseFloorMax,
				want: `≤ ${profile.noiseFloorMax}`
			});
		}
		return out;
	});

	let measured = $derived(rms > LUFS_SILENT);

	// Both families are always on screen; the profile only draws lines over them.
	let lufsBars = $derived([
		{ label: 'M', val: momentary, hold: holdM as number | null },
		{ label: 'S', val: shortterm, hold: holdS as number | null },
		{ label: 'I', val: integrated, hold: holdI as number | null }
	]);

	let rmsBars = $derived([
		{ label: 'RMS', val: rms, hold: null as number | null },
		{ label: 'Floor', val: noiseFloor, hold: null as number | null }
	]);

	function resetPeaks() {
		holdM = LUFS_SILENT;
		holdS = LUFS_SILENT;
		holdI = LUFS_SILENT;
		tpMax = LUFS_SILENT;
	}

	let unlisten: UnlistenFn | undefined;
	let unlistenReset: (() => void) | undefined;

	onMount(async () => {
		unlistenReset = onNodeAction(id, 'resetPeaks', () => resetPeaks());
		unlisten = await listen<LufsTick>('audio://lufs', (event) => {
			const p = event.payload;
			if (p.nodeId !== id) return;
			momentary = p.momentary;
			shortterm = p.shortterm;
			integrated = p.integrated;
			holdM = Math.max(holdM, momentary);
			holdS = Math.max(holdS, shortterm);
			holdI = Math.max(holdI, integrated);
			tpMax = Math.max(tpMax, p.tpL, p.tpR);
			lra = p.lra;
			rms = p.rms;
			noiseFloor = p.noiseFloor;
			samplePeak = p.samplePeak;
			dcOffset = p.dcOffset;
			correlation = p.correlation;
			clips = p.clips;
		});
	});

	onDestroy(() => {
		unlisten?.();
		unlistenReset?.();
	});
</script>

<Wrapper
	label="Loudness"
	icon={Gauge}
	accent="monitor"
	hasInput
	hasOutput
	channelIo
	nodeId={id}
	maxChannels={2}
	wide
>
	<div class="flex w-fit flex-col gap-1.5 font-mono text-[10px]">
		<div class="flex gap-3">
			{#snippet group(
				bars: { label: string; val: number; hold: number | null }[],
				floor: number,
				ticks: number[],
				target: number | null,
				zone: [number, number] | null,
				title: string,
				unit: string
			)}
				<div class="flex flex-col gap-0.5">
					<div class="flex gap-1.5">
						<button
							type="button"
							onclick={resetPeaks}
							{title}
							class="nodrag nopan relative flex items-stretch overflow-hidden rounded-sm border border-neutral-300"
							style="width: {BAR_W * bars.length}px; height: {BAR_H}px;"
						>
							{#each bars as bar, i (bar.label)}
								<MeterBar
									class="flex-1 {i > 0 ? 'border-l border-neutral-300' : ''}"
									orientation="vertical"
									gradient={LUFS_GRADIENT}
									ghost
									hover
									hoverLabel={hoverLabel(floor)}
									pct={dbToPct(bar.val, floor)}
									hold={bar.hold != null && bar.hold > LUFS_SILENT
										? dbToPct(bar.hold, floor)
										: null}
								/>
							{/each}

							{#if zone}
								<div
									class="pointer-events-none absolute right-0 left-0 border-y border-green-600/70 bg-green-500/15"
									style="bottom: {dbToPct(zone[0], floor)}%; height: {dbToPct(
										zone[1],
										floor
									) - dbToPct(zone[0], floor)}%;"
								></div>
							{:else if target != null}
								<div
									class="pointer-events-none absolute right-0 left-0 h-px bg-neutral-900"
									style="bottom: {dbToPct(target, floor)}%;"
								>
									<div
										class="absolute top-1/2 -right-px h-1.5 w-1.5 translate-x-full -translate-y-1/2 rotate-45 bg-neutral-900"
									></div>
								</div>
							{/if}
						</button>

						<div
							class="relative w-7 text-[8px] text-neutral-700 select-none"
							style="height: {BAR_H}px;"
						>
							{#each ticks as db (db)}
								<div
									class="absolute left-0 flex items-center"
									style="bottom: {dbToPct(db, floor)}%; height: 1px;"
								>
									<div class="h-px w-1.5 shrink-0 bg-neutral-400"></div>
									<span class="ml-0.5 leading-none">{db}</span>
								</div>
							{/each}
						</div>
					</div>

					<div
						class="flex overflow-hidden rounded-sm border border-neutral-300 bg-neutral-100"
						style="width: {BAR_W * bars.length}px;"
					>
						{#each bars as bar, i (bar.label)}
							<div
								class={[
									'flex flex-1 flex-col items-center py-0.5',
									i > 0 && 'border-l border-neutral-300'
								]}
							>
								<span class="text-[7px] leading-none text-neutral-500"
									>{bar.label}</span
								>
								<span class="text-[8px] leading-tight tabular-nums"
									>{format(bar.val)}</span
								>
								<span class="text-[7px] leading-none text-neutral-400">{unit}</span>
							</div>
						{/each}
					</div>
				</div>
			{/snippet}

			<div class="flex flex-col gap-0.5">
				{@render group(
					lufsBars,
					LUFS_FLOOR,
					LUFS_TICKS,
					profile.target ?? null,
					null,
					'Reset peaks',
					'LUFS'
				)}
				<div
					class="h-3 text-center text-[8px] leading-3 font-semibold {targetClass(
						integrated,
						profile.target
					)}"
					style="width: {BAR_W * 3}px;"
				>
					{targetDelta(integrated, profile.target) ?? ''}
				</div>
			</div>

			{@render group(
				rmsBars,
				RMS_FLOOR,
				RMS_TICKS,
				null,
				profile.rmsRange ?? null,
				'Reset peaks',
				'dBFS'
			)}

			<!-- Program stats -->
			<div class="flex flex-col gap-1" style="width: {BAR_W * 2}px;">
				{#each [{ label: 'True Peak', unit: 'dBTP', val: format(tpMax) }, { label: 'Loudness Range', unit: 'LU', val: lra.toFixed(1) }, { label: 'Peak / Loudness', unit: 'LU', val: ratio(tpMax, integrated) }, { label: 'Dynamic Range', unit: 'LU', val: ratio(tpMax, shortterm) }, { label: 'Sample Peak', unit: 'dBFS', val: format(samplePeak) }, { label: 'DC Offset', unit: 'dBFS', val: formatDc(dcOffset) }, { label: 'Correlation', unit: '', val: correlation.toFixed(2) }, { label: 'Clipped', unit: 'smp', val: String(clips) }] as stat (stat.label)}
					<div
						class="flex flex-col rounded-sm border border-neutral-300 bg-neutral-100 px-1.5 py-1"
					>
						<span class="text-[7px] leading-none text-neutral-500">{stat.label}</span>
						<span
							class="text-[11px] leading-tight font-semibold text-neutral-900 tabular-nums"
						>
							{stat.val}<span class="ml-0.5 text-[7px] font-normal text-neutral-400"
								>{stat.unit}</span
							>
						</span>
					</div>
				{/each}
			</div>
		</div>

		<button
			type="button"
			onclick={resetPeaks}
			class="nodrag nopan flex items-center justify-center gap-1 rounded-sm border border-neutral-300 bg-neutral-100 py-1 text-[8px] text-neutral-900 transition-colors hover:bg-neutral-200 hover:text-theme"
			title="Clear held peaks and the true-peak maximum"
		>
			<Refresh class="size-2.5" />
			Reset peaks
		</button>

		<!-- delivery profile -->
		<div class="flex flex-col gap-0.5">
			<Combobox
				options={PROFILES.map((p) => ({ value: p.id, label: p.label }))}
				value={profile.id}
				size="sm"
				onChange={setProfile}
			/>
			<span class="text-[8px] leading-tight text-neutral-500">{profile.note}</span>
		</div>

		{#if checks.length > 0}
			<div class="flex flex-col gap-0.5">
				{#each checks as c (c.label)}
					<div
						class="flex items-center gap-1.5 rounded-sm border border-neutral-300 bg-neutral-100 px-1.5 py-1"
					>
						<span
							class={[
								'size-1.5 shrink-0 rounded-full',
								!measured ? 'bg-neutral-400' : c.ok ? 'bg-green-500' : 'bg-red-500'
							]}
						></span>
						<span class="flex-1 text-[8px] text-neutral-900">{c.label}</span>
						<span class="text-[8px] tabular-nums text-neutral-500">{c.want}</span>
					</div>
				{/each}
			</div>
		{/if}
	</div>
</Wrapper>
