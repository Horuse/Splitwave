<script lang="ts">
	import { createId } from '@paralleldrive/cuid2';
	import { onMount } from 'svelte';
	import { audioStore } from '$lib/modules/audio/stores.svelte';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import type { NativeDeviceInfo } from '$lib/modules/audio/types';
	import { modalManager, type ModalBaseProps } from '$lib/modules/overlay/modal';
	import type { MicrophoneArrayGeometry, MicrophoneArrayMember, MicrophoneArrayNodeData, MicrophoneArraySource } from '$lib/modules/pipeline/types';

	type Props = ModalBaseProps & {
		data: MicrophoneArrayNodeData;
		onCalibrate: (data: MicrophoneArrayNodeData) => Promise<MicrophoneArrayNodeData>;
	};
	function cloneData(value: MicrophoneArrayNodeData) {
		return structuredClone($state.snapshot(value));
	}

	let { modalId, data, onCalibrate }: Props = $props();
	// svelte-ignore state_referenced_locally -- each modal owns one isolated draft
	let draft = $state<MicrophoneArrayNodeData>(cloneData(data));
	let deviceInfo = $state<Record<string, NativeDeviceInfo>>({});
	let loadingDevices = $state(false);
	let calibrating = $state(false);
	let calibrationError = $state<string | null>(null);

	let enabledMembers = $derived(draft.members.filter((member) => member.enabled && member.quality !== 'excluded').length);
	let independentClocks = $derived(draft.sources.length > 1);
	let estimatedCpu = $derived.by(() => {
		if (draft.algorithm === 'delayAndSum') return Math.max(0.1, enabledMembers * 0.025);
		if (draft.algorithm === 'gsc') return Math.max(0.2, enabledMembers * 0.06);
		return Math.max(0.3, enabledMembers * 0.095);
	});
	let validation = $derived.by(() => {
		if (draft.sources.length === 0) return 'Add at least one physical input.';
		if (draft.sources.some((source) => !source.deviceId)) return 'Choose a device for every physical input.';
		if (new Set(draft.sources.map((source) => source.deviceId)).size !== draft.sources.length) return 'A physical device can only be opened once.';
		if (enabledMembers < 2) return 'Select at least two usable microphone channels.';
		if (independentClocks && !draft.masterSourceId) return 'Choose the master clock device.';
		if (
			!Number.isFinite(draft.processingSampleRate) ||
			!Number.isFinite(draft.strength) ||
			!Number.isFinite(draft.maxAttenuationDb) ||
			draft.strength < 0 ||
			draft.strength > 1 ||
			draft.maxAttenuationDb < 0 ||
			draft.maxAttenuationDb > 36
		)
			return 'Processing controls are outside their safe bounds.';
		if (draft.members.some((member) => Object.values(member.position).some((coordinate) => !Number.isFinite(coordinate)))) {
			return 'Every microphone position must be a finite number.';
		}
		return null;
	});

	onMount(async () => {
		loadingDevices = true;
		try {
			await audioStore.refreshInputDevices();
			await Promise.all(draft.sources.map((source) => loadInfo(source)));
		} finally {
			loadingDevices = false;
		}
	});

	function staleCalibration() {
		draft.calibration = {
			state: 'missing',
			fingerprint: null,
			residualDelaySamples: null,
			qualityScore: null
		};
		calibrationError = null;
	}

	async function rescan() {
		loadingDevices = true;
		try {
			await audioStore.refreshInputDevices();
			deviceInfo = {};
			await Promise.all(draft.sources.map((source) => loadInfo(source)));
		} finally {
			loadingDevices = false;
		}
	}

	async function loadInfo(source: MicrophoneArraySource) {
		if (!source.deviceId) return;
		const info = await audioMethods.deviceInfo('input', source.deviceId).catch(() => null);
		if (info) deviceInfo[source.id] = info;
	}

	function addSource() {
		const source: MicrophoneArraySource = {
			id: createId(),
			deviceId: null,
			label: `Input ${draft.sources.length + 1}`
		};
		draft.sources = [...draft.sources, source];
		if (!draft.masterSourceId) draft.masterSourceId = source.id;
		staleCalibration();
	}

	function removeSource(sourceId: string) {
		draft.sources = draft.sources.filter((source) => source.id !== sourceId);
		draft.members = draft.members.filter((member) => member.sourceId !== sourceId);
		delete deviceInfo[sourceId];
		if (draft.masterSourceId === sourceId) draft.masterSourceId = draft.sources[0]?.id ?? null;
		applyGeometry();
		staleCalibration();
	}

	async function setDevice(sourceId: string, deviceId: string | null) {
		draft.sources = draft.sources.map((source) => (source.id === sourceId ? { ...source, deviceId } : source));
		draft.members = draft.members.filter((member) => member.sourceId !== sourceId);
		delete deviceInfo[sourceId];
		const source = draft.sources.find((candidate) => candidate.id === sourceId);
		if (source) await loadInfo(source);
		applyGeometry();
		staleCalibration();
	}

	function setSourceLabel(sourceId: string, label: string) {
		draft.sources = draft.sources.map((source) => (source.id === sourceId ? { ...source, label } : source));
	}

	function selected(sourceId: string, channelIndex: number): boolean {
		return draft.members.some((member) => member.sourceId === sourceId && member.channelIndex === channelIndex);
	}

	function toggleChannel(source: MicrophoneArraySource, channelIndex: number) {
		const existing = draft.members.findIndex((member) => member.sourceId === source.id && member.channelIndex === channelIndex);
		if (existing >= 0) {
			draft.members = draft.members.filter((_, index) => index !== existing);
		} else {
			const member: MicrophoneArrayMember = {
				sourceId: source.id,
				channelIndex,
				label: `Mic ${draft.members.length + 1}`,
				position: { x: 0, y: 0, z: 0 },
				enabled: true,
				weight: 1,
				gainDb: 0,
				polarityInverted: false,
				fixedDelaySamples: 0,
				quality: 'good',
				exclusionReason: null
			};
			draft.members = [...draft.members, member];
		}
		applyGeometry();
		staleCalibration();
	}

	function updateMember(index: number, patch: Partial<MicrophoneArrayMember>, structural = false) {
		draft.members = draft.members.map((member, memberIndex) => (memberIndex === index ? { ...member, ...patch } : member));
		if (structural) staleCalibration();
	}

	function setGeometry(kind: MicrophoneArrayGeometry['kind']) {
		if (kind === 'linear') draft.geometry = { kind, spacing_m: 0.04, orientation_degrees: 0 };
		else if (kind === 'circular') draft.geometry = { kind, radius_m: 0.06, rotation_degrees: 0 };
		else if (kind === 'rectangular') {
			draft.geometry = { kind, rows: 2, columns: 2, horizontal_spacing_m: 0.04, vertical_spacing_m: 0.04, rotation_degrees: 0 };
		} else draft.geometry = { kind: 'custom' };
		applyGeometry();
		staleCalibration();
	}

	function patchGeometry(patch: Record<string, number>) {
		if (Object.values(patch).some((value) => !Number.isFinite(value))) return;
		draft.geometry = { ...draft.geometry, ...patch } as MicrophoneArrayGeometry;
		applyGeometry();
		staleCalibration();
	}

	function applyGeometry() {
		const count = draft.members.length;
		if (count === 0 || draft.geometry.kind === 'custom') return;
		const rotation =
			draft.geometry.kind === 'linear'
				? draft.geometry.orientation_degrees
				: draft.geometry.kind === 'circular' || draft.geometry.kind === 'rectangular'
					? draft.geometry.rotation_degrees
					: 0;
		const radians = (rotation * Math.PI) / 180;
		const rotate = (x: number, y: number) => ({
			x: x * Math.cos(radians) - y * Math.sin(radians),
			y: x * Math.sin(radians) + y * Math.cos(radians),
			z: 0
		});
		draft.members = draft.members.map((member, index) => {
			let position = { x: 0, y: 0, z: 0 };
			if (draft.geometry.kind === 'linear') {
				position = rotate((index - (count - 1) / 2) * draft.geometry.spacing_m, 0);
			} else if (draft.geometry.kind === 'circular') {
				const angle = (2 * Math.PI * index) / count;
				position = rotate(Math.cos(angle) * draft.geometry.radius_m, Math.sin(angle) * draft.geometry.radius_m);
			} else if (draft.geometry.kind === 'rectangular') {
				const columns = Math.max(1, draft.geometry.columns);
				const row = Math.floor(index / columns);
				const column = index % columns;
				position = rotate(
					(column - (columns - 1) / 2) * draft.geometry.horizontal_spacing_m,
					(row - (Math.max(1, draft.geometry.rows) - 1) / 2) * draft.geometry.vertical_spacing_m
				);
			}
			return { ...member, position };
		});
	}

	function setTargetField(field: 'azimuth_degrees' | 'elevation_degrees', value: number) {
		if (draft.target.kind !== 'direction' || !Number.isFinite(value)) return;
		draft.target = { ...draft.target, [field]: value };
		staleCalibration();
	}

	function setMasterSource(sourceId: string) {
		if (draft.masterSourceId === sourceId) return;
		draft.masterSourceId = sourceId;
		staleCalibration();
	}

	async function calibrate() {
		if (validation || calibrating) return;
		calibrating = true;
		calibrationError = null;
		try {
			if (await audioMethods.isPipelineRunning()) throw new Error('Stop the running pipeline before calibrating this array.');
			draft = await onCalibrate(structuredClone($state.snapshot(draft)));
		} catch (error) {
			calibrationError = error instanceof Error ? error.message : String(error);
		} finally {
			calibrating = false;
		}
	}

	function save() {
		if (!validation) modalManager.close(modalId, structuredClone($state.snapshot(draft)));
	}

	function formatRate(rate: number): string {
		return `${rate / 1000} kHz`;
	}
</script>

<div class="grid min-h-0 grid-cols-[15rem_minmax(0,1fr)] gap-0 pt-3">
	<aside class="border-r border-neutral-300 px-5 pb-5">
		<div class="sticky top-0 flex flex-col gap-4">
			<div class="rounded-xl border border-neutral-300 bg-neutral-200/60 p-3">
				<div class="flex items-center justify-between text-[10px] font-semibold tracking-wider text-neutral-900 uppercase">
					<span>Array status</span>
					<span class={['size-2 rounded-full', validation ? 'bg-amber-500' : draft.calibration.state === 'ready' ? 'bg-emerald-500' : 'bg-sky-500']}
					></span>
				</div>
				<div class="mt-3 grid grid-cols-2 gap-2 font-mono text-[10px] tabular-nums">
					<div><span class="block text-neutral-700">MICS</span><span class="text-sm text-theme">{enabledMembers}</span></div>
					<div><span class="block text-neutral-700">CLOCKS</span><span class="text-sm text-theme">{draft.sources.length}</span></div>
					<div>
						<span class="block text-neutral-700">LATENCY</span><span class="text-sm text-theme"
							>{draft.algorithm === 'delayAndSum' ? '0' : '5.3'} ms</span>
					</div>
					<div><span class="block text-neutral-700">EST. CPU</span><span class="text-sm text-theme">{estimatedCpu.toFixed(1)}%</span></div>
				</div>
			</div>

			<nav class="flex flex-col gap-1 text-xs">
				<a class="rounded-lg px-2.5 py-1.5 text-neutral-900 hover:bg-neutral-200" href="#array-sources">Sources</a>
				<a class="rounded-lg px-2.5 py-1.5 text-neutral-900 hover:bg-neutral-200" href="#array-geometry">Geometry & target</a>
				<a class="rounded-lg px-2.5 py-1.5 text-neutral-900 hover:bg-neutral-200" href="#array-calibration">Calibration</a>
				<a class="rounded-lg px-2.5 py-1.5 text-neutral-900 hover:bg-neutral-200" href="#array-processing">Processing</a>
				<a class="rounded-lg px-2.5 py-1.5 text-neutral-900 hover:bg-neutral-200" href="#array-diagnostics">Diagnostics</a>
			</nav>

			{#if independentClocks}
				<div class="rounded-xl border border-amber-500/30 bg-amber-500/10 p-3 text-[10px] leading-relaxed text-amber-900">
					<div class="font-semibold tracking-wide uppercase">Experimental</div>
					Independent USB devices need continuous clock recovery and add roughly 64 ms of safety buffering.
				</div>
			{/if}
		</div>
	</aside>

	<main class="min-w-0 space-y-6 px-5 pb-5">
		<section id="array-sources" class="scroll-mt-4 space-y-3">
			<div class="flex items-end justify-between gap-3">
				<div>
					<h3 class="text-xs font-semibold text-theme">Physical sources</h3>
					<p class="mt-0.5 text-[10px] text-neutral-800">One stream per device; select any number of its native channels.</p>
				</div>
				<div class="flex gap-2">
					<button type="button" class="button-main secondary h-7 rounded-lg px-3 text-[10px]" disabled={loadingDevices} onclick={rescan}
						>{loadingDevices ? 'Scanning…' : 'Rescan'}</button>
					<button type="button" class="button-main primary h-7 rounded-lg px-3 text-[10px]" onclick={addSource}>Add input</button>
				</div>
			</div>

			{#if draft.sources.length === 0}
				<button
					type="button"
					onclick={addSource}
					class="flex w-full flex-col items-center justify-center gap-1 rounded-xl border border-dashed border-neutral-400 bg-neutral-200/30 px-4 py-8 text-center hover:bg-neutral-200/60">
					<span class="text-xs font-medium text-theme">Add the first audio device</span><span class="text-[10px] text-neutral-800"
						>Audio interfaces with multiple inputs are recommended.</span>
				</button>
			{/if}

			{#each draft.sources as source, sourceIndex (source.id)}
				<div class="rounded-xl border border-neutral-300 bg-neutral-200/40 p-3">
					<div class="grid grid-cols-[8rem_minmax(0,1fr)_auto] items-center gap-2">
						<input
							class="input-base h-8 text-xs"
							value={source.label}
							aria-label="Source label"
							oninput={(event) => setSourceLabel(source.id, event.currentTarget.value)} />
						<select
							class="input-base h-8 min-w-0 text-xs"
							value={source.deviceId ?? ''}
							onchange={(event) => setDevice(source.id, event.currentTarget.value || null)}>
							<option value="">Select an input device…</option>
							{#each audioStore.inputDevices as device (device.id)}
								<option value={device.id} disabled={draft.sources.some((other) => other.id !== source.id && other.deviceId === device.id)}
									>{device.name}</option>
							{/each}
						</select>
						<button type="button" class="button-main red h-8 rounded-lg px-2 text-[10px]" onclick={() => removeSource(source.id)}>Remove</button>
					</div>

					{#if source.deviceId && deviceInfo[source.id]}
						{@const info = deviceInfo[source.id]}
						<div class="mt-3 flex items-center justify-between gap-3">
							<div class="flex flex-wrap gap-1.5">
								{#each Array(info.channels) as _, channelIndex}
									<button
										type="button"
										class={[
											'h-7 rounded-lg border px-2.5 font-mono text-[10px] transition-colors',
											selected(source.id, channelIndex)
												? 'border-emerald-500 bg-emerald-500/15 text-emerald-800'
												: 'border-neutral-400 bg-neutral-100 text-neutral-900 hover:bg-neutral-200'
										]}
										onclick={() => toggleChannel(source, channelIndex)}>CH {channelIndex + 1}</button>
								{/each}
							</div>
							<span class="shrink-0 font-mono text-[9px] text-neutral-700"
								>{formatRate(info.sampleRate)} · {info.channels} ch · {info.sampleFormat}</span>
						</div>
					{/if}

					{#if independentClocks}
						<label class="mt-3 flex items-center gap-2 text-[10px] text-neutral-900"
							><input type="radio" name="master-clock" checked={draft.masterSourceId === source.id} onchange={() => setMasterSource(source.id)} />
							Master clock {sourceIndex === 0 ? '(recommended)' : ''}</label>
					{/if}
				</div>
			{/each}
		</section>

		<section id="array-geometry" class="scroll-mt-4 space-y-3 border-t border-neutral-300 pt-5">
			<div>
				<h3 class="text-xs font-semibold text-theme">Geometry & target</h3>
				<p class="mt-0.5 text-[10px] text-neutral-800">
					Positions are in metres. Presets keep arbitrary channel counts and can be converted to Custom.
				</p>
			</div>
			<div class="grid grid-cols-[10rem_1fr] gap-3">
				<select
					class="input-base h-8 text-xs"
					value={draft.geometry.kind}
					onchange={(event) => setGeometry(event.currentTarget.value as MicrophoneArrayGeometry['kind'])}>
					<option value="linear">Linear</option><option value="circular">Circular</option><option value="rectangular">Rectangular</option><option
						value="custom">Custom</option>
				</select>
				<div class="grid grid-cols-3 gap-2">
					{#if draft.geometry.kind === 'linear'}
						<label class="field-label"
							>Spacing, m<input
								class="input-base h-8 font-mono text-xs"
								type="number"
								min="0.001"
								step="0.005"
								value={draft.geometry.spacing_m}
								oninput={(event) => patchGeometry({ spacing_m: event.currentTarget.valueAsNumber })} /></label>
						<label class="field-label"
							>Rotation, °<input
								class="input-base h-8 font-mono text-xs"
								type="number"
								step="1"
								value={draft.geometry.orientation_degrees}
								oninput={(event) => patchGeometry({ orientation_degrees: event.currentTarget.valueAsNumber })} /></label>
					{:else if draft.geometry.kind === 'circular'}
						<label class="field-label"
							>Radius, m<input
								class="input-base h-8 font-mono text-xs"
								type="number"
								min="0.001"
								step="0.005"
								value={draft.geometry.radius_m}
								oninput={(event) => patchGeometry({ radius_m: event.currentTarget.valueAsNumber })} /></label>
						<label class="field-label"
							>Rotation, °<input
								class="input-base h-8 font-mono text-xs"
								type="number"
								step="1"
								value={draft.geometry.rotation_degrees}
								oninput={(event) => patchGeometry({ rotation_degrees: event.currentTarget.valueAsNumber })} /></label>
					{:else if draft.geometry.kind === 'rectangular'}
						<label class="field-label"
							>Rows<input
								class="input-base h-8 font-mono text-xs"
								type="number"
								min="1"
								step="1"
								value={draft.geometry.rows}
								oninput={(event) => patchGeometry({ rows: event.currentTarget.valueAsNumber })} /></label>
						<label class="field-label"
							>Columns<input
								class="input-base h-8 font-mono text-xs"
								type="number"
								min="1"
								step="1"
								value={draft.geometry.columns}
								oninput={(event) => patchGeometry({ columns: event.currentTarget.valueAsNumber })} /></label>
						<label class="field-label"
							>Spacing X, m<input
								class="input-base h-8 font-mono text-xs"
								type="number"
								min="0.001"
								step="0.005"
								value={draft.geometry.horizontal_spacing_m}
								oninput={(event) => patchGeometry({ horizontal_spacing_m: event.currentTarget.valueAsNumber })} /></label>
					{/if}
				</div>
			</div>

			<div class="overflow-hidden rounded-xl border border-neutral-300">
				<div class="grid grid-cols-[2rem_1fr_5rem_5rem_5rem_4rem] gap-2 bg-neutral-200/80 px-3 py-2 font-mono text-[9px] text-neutral-700">
					<span></span><span>MEMBER</span><span>X</span><span>Y</span><span>Z</span><span>GAIN</span>
				</div>
				{#each draft.members as member, index (`${member.sourceId}:${member.channelIndex}`)}
					<div class="grid grid-cols-[2rem_1fr_5rem_5rem_5rem_4rem] items-center gap-2 border-t border-neutral-300 px-3 py-2">
						<input
							type="checkbox"
							checked={member.enabled}
							onchange={(event) => updateMember(index, { enabled: event.currentTarget.checked }, true)}
							aria-label="Enable member" />
						<div class="min-w-0">
							<input
								class="input-base h-7 w-full text-[10px]"
								value={member.label}
								oninput={(event) => updateMember(index, { label: event.currentTarget.value })} /><span
								class="mt-0.5 block truncate font-mono text-[8px] text-neutral-700"
								>{draft.sources.find((source) => source.id === member.sourceId)?.label} · CH {member.channelIndex + 1}</span>
						</div>
						{#each ['x', 'y', 'z'] as axis}
							<input
								class="input-base h-7 font-mono text-[10px]"
								type="number"
								step="0.005"
								value={member.position[axis as keyof typeof member.position]}
								disabled={draft.geometry.kind !== 'custom'}
								oninput={(event) => updateMember(index, { position: { ...member.position, [axis]: event.currentTarget.valueAsNumber } }, true)}
								aria-label={`${axis} position`} />
						{/each}
						<span class="font-mono text-[9px] text-neutral-900 tabular-nums">{member.gainDb.toFixed(1)} dB</span>
					</div>
				{/each}
			</div>

			<div class="grid grid-cols-3 gap-2 rounded-xl border border-neutral-300 bg-neutral-200/40 p-3">
				<label class="field-label"
					>Target<select
						class="input-base h-8 text-xs"
						value={draft.target.kind}
						onchange={() => {
							draft.target = { kind: 'direction', azimuth_degrees: 90, elevation_degrees: 0 };
							staleCalibration();
						}}><option value="direction">Fixed direction</option></select
					></label>
				{#if draft.target.kind === 'direction'}
					<label class="field-label"
						>Azimuth, °<input
							class="input-base h-8 font-mono text-xs"
							type="number"
							min="-180"
							max="180"
							step="1"
							value={draft.target.azimuth_degrees}
							oninput={(event) => setTargetField('azimuth_degrees', event.currentTarget.valueAsNumber)} /></label>
					<label class="field-label"
						>Elevation, °<input
							class="input-base h-8 font-mono text-xs"
							type="number"
							min="-90"
							max="90"
							step="1"
							value={draft.target.elevation_degrees}
							oninput={(event) => setTargetField('elevation_degrees', event.currentTarget.valueAsNumber)} /></label>
				{/if}
			</div>
		</section>

		<section id="array-calibration" class="scroll-mt-4 space-y-3 border-t border-neutral-300 pt-5">
			<div>
				<h3 class="text-xs font-semibold text-theme">Calibration</h3>
				<p class="mt-0.5 text-[10px] text-neutral-800">
					Play speech or broadband noise from the target direction for three seconds. Splitwave measures delay, gain, polarity and channel quality.
				</p>
			</div>
			<div class="flex items-center justify-between gap-4 rounded-xl border border-neutral-300 bg-neutral-200/40 p-3">
				<div>
					<div class="flex items-center gap-2">
						<span
							class={[
								'size-2 rounded-full',
								draft.calibration.state === 'ready'
									? 'bg-emerald-500'
									: draft.calibration.state === 'needsReview'
										? 'bg-amber-500'
										: 'bg-neutral-500'
							]}></span
						><span class="text-xs font-medium text-theme"
							>{draft.calibration.state === 'ready'
								? 'Calibration ready'
								: draft.calibration.state === 'needsReview'
									? 'Calibration needs review'
									: 'Not calibrated'}</span>
					</div>
					<div class="mt-1 font-mono text-[9px] text-neutral-700">
						QUALITY {draft.calibration.qualityScore ?? '—'} · RESIDUAL {draft.calibration.residualDelaySamples?.toFixed(2) ?? '—'} samples
					</div>
				</div>
				<button
					type="button"
					class="button-main green h-8 rounded-lg px-4 text-[10px] font-semibold"
					disabled={!!validation || calibrating}
					onclick={calibrate}>{calibrating ? 'Listening…' : 'Calibrate 3 s'}</button>
			</div>
			{#if calibrationError}<div class="rounded-lg border border-red-500/30 bg-red-500/10 px-3 py-2 text-[10px] text-red-700">
					{calibrationError}
				</div>{/if}
		</section>

		<section id="array-processing" class="scroll-mt-4 space-y-3 border-t border-neutral-300 pt-5">
			<div>
				<h3 class="text-xs font-semibold text-theme">Processing</h3>
				<p class="mt-0.5 text-[10px] text-neutral-800">Auto uses calibration and sync health to choose MVDR or the safe delay-and-sum fallback.</p>
			</div>
			<div class="grid grid-cols-2 gap-3 rounded-xl border border-neutral-300 bg-neutral-200/40 p-3">
				<label class="field-label"
					>Algorithm<select class="input-base h-8 text-xs" bind:value={draft.algorithm}
						><option value="auto">Auto</option><option value="delayAndSum">Delay-and-sum</option><option value="gsc">GSC</option><option
							value="mvdr">MVDR</option
						></select
					></label>
				<label class="field-label"
					>Strength <span class="font-mono tabular-nums">{Math.round(draft.strength * 100)}%</span><input
						class="mt-2 w-full accent-emerald-600"
						type="range"
						min="0"
						max="1"
						step="0.01"
						bind:value={draft.strength} /></label>
				<label class="field-label"
					>Max attenuation, dB<input
						class="input-base h-8 font-mono text-xs"
						type="number"
						min="0"
						max="36"
						step="1"
						bind:value={draft.maxAttenuationDb} /></label>
				<div class="flex items-end gap-4 pb-1 text-[10px] text-neutral-900">
					<label class="flex items-center gap-2"><input type="checkbox" bind:checked={draft.postfilterEnabled} /> Spatial postfilter</label><label
						class="flex items-center gap-2"><input type="checkbox" bind:checked={draft.limiterEnabled} /> Safety limiter</label>
				</div>
			</div>
		</section>

		<section id="array-diagnostics" class="scroll-mt-4 space-y-3 border-t border-neutral-300 pt-5">
			<div>
				<h3 class="text-xs font-semibold text-theme">Test & diagnostics</h3>
				<p class="mt-0.5 text-[10px] text-neutral-800">
					Use the graph Run control for a live test. Bypass crossfades to the best healthy microphone without changing the graph.
				</p>
			</div>
			<div class="grid grid-cols-4 gap-2">
				{#each [['Topology', independentClocks ? `${draft.sources.length} clocks` : 'shared clock'], ['Rate', formatRate(draft.processingSampleRate)], ['Output', 'mono'], ['Fallback', 'best healthy mic']] as diagnostic}
					<div class="rounded-lg border border-neutral-300 bg-neutral-200/40 px-2.5 py-2">
						<span class="block text-[9px] text-neutral-700">{diagnostic[0]}</span><span class="mt-0.5 block font-mono text-[10px] text-theme"
							>{diagnostic[1]}</span>
					</div>
				{/each}
			</div>
		</section>
	</main>
</div>

<div class="sticky bottom-0 flex items-center justify-between border-t border-neutral-300 bg-neutral-100 px-5 py-3">
	<div class={['min-w-0 text-[10px]', validation ? 'text-amber-800' : 'text-emerald-700']}>{validation ?? 'Configuration is ready to save.'}</div>
	<div class="flex shrink-0 gap-2">
		<button type="button" class="button-main primary h-8 rounded-lg px-4 text-xs" onclick={() => modalManager.close(modalId)}>Cancel</button><button
			type="button"
			class="button-main green h-8 rounded-lg px-4 text-xs font-semibold"
			disabled={!!validation || calibrating}
			onclick={save}>Apply setup</button>
	</div>
</div>

<style>
	.field-label {
		display: flex;
		min-width: 0;
		flex-direction: column;
		gap: 0.3rem;
		font-size: 0.625rem;
		color: var(--color-neutral-900);
	}
</style>
