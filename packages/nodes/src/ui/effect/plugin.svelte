<script lang="ts">
	import { untrack } from 'svelte';
	import { Button, Combobox, Input, Knob, RescanButton, ScrollArea, Slider, Toggle, Tooltip } from '@splitwave/ui';
	import { slide } from 'svelte/transition';
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { PluginNodeData } from '@splitwave/nodes';
	import Wrapper from '../node.svelte';
	import Plug from '~icons/fluent/plug-connected-16-filled';
	import Refresh from '~icons/fluent/arrow-clockwise-16-filled';
	import WindowOff from '~icons/fluent/window-multiple-off-16-regular';
	import WindowNew from '~icons/fluent/window-new-16-regular';
	import PinOn from '~icons/fluent/pin-12-filled';
	import PinOff from '~icons/fluent/pin-12-regular';
	import Chevron from '~icons/fluent/chevron-right-12-filled';
	import PluginPresets from './_plugin_presets.svelte';
	import { pluginSidechainHandle } from '../utils';
	import { getEnginePort, type PluginParam, type PluginStatus } from '../../engine';

	const engine = getEnginePort();

	type PluginNodeType = Node<PluginNodeData, 'plugin'>;
	let { id, data }: NodeProps<PluginNodeType> = $props();

	const flow = useSvelteFlow();

	// uid is the stable scan key; node data stores path + pluginId.
	const currentUid = $derived(engine.plugins.plugins.find((p) => p.path === data.path && p.pluginId === data.pluginId)?.uid ?? null);

	// Known only once a scan has run: before that the stored name stands in.
	const missing = $derived(engine.plugins.scanned && !!data.path && !currentUid);

	$effect(() => {
		engine.plugins.ensure();
	});

	const noPlugins = $derived(engine.plugins.scanned && !engine.plugins.scanning && engine.plugins.plugins.length === 0 && !data.path);

	const options = $derived(
		engine.plugins.plugins.map((p) => ({
			value: p.uid,
			label: p.name,
			subtitle: p.vendor || null,
			badge: p.format
		}))
	);

	function select(uid: string | null) {
		const desc = engine.plugins.plugins.find((p) => p.uid === uid);
		flow.updateNodeData(id, {
			// The engine dispatches on this, so it is stored rather than guessed
			// from the path shape.
			format: desc?.format ?? null,
			path: desc?.path ?? '',
			pluginId: desc?.pluginId ?? '',
			name: desc?.name ?? '',
			vendor: desc?.vendor ?? '',
			// Drop the previous plugin's state; loading it into a different plugin
			// feeds garbage to its state parser (some plugins panic on it).
			state: null,
			// The new plugin's buses are unknown until it answers below; the old
			// plugin's sockets must not stay on the node meanwhile.
			layout: null
		});
	}

	// The sockets come from what the plugin declares, so they exist before the
	// pipeline runs. A node saved before this was stored asks once on load.
	// Deriveds, not `data`: every updateNodeData hands the node a new data
	// object, which would rerun the probe.
	const pluginFormat = $derived(data.format);
	const pluginUid = $derived(data.pluginId);
	const hasLayout = $derived(!!data.layout);
	let probeError = $state<string | null>(null);

	$effect(() => {
		const format = pluginFormat;
		const path = pluginPath;
		const pluginId = pluginUid;
		probeError = null;
		if (!format || !path || hasLayout) return;
		let current = true;
		engine.audio
			.probePluginPorts(format, path, pluginId)
			.then((layout) => {
				if (current) flow.updateNodeData(id, { layout });
			})
			.catch((e) => {
				if (current) probeError = String(e);
			});
		return () => (current = false);
	});

	// Every input that is not a main bus is a socket of its own, labelled with
	// the plugin's name for it. The engine counts them the same way: by role, in
	// the plugin's own order.
	const extraInputs = $derived(
		(data.layout?.inputs ?? [])
			.filter((bus) => !bus.main)
			.map((bus, i, all) => ({
				id: pluginSidechainHandle(i + 1),
				label: bus.name || `Sidechain ${i + 1}`,
				position: 'bottom' as const,
				offset: (i + 1) / (all.length + 1)
			}))
	);

	let editorOpen = $state(false);

	// A node wider than the plugin runs one copy of it per group of channels, so
	// every channel is processed. On by default, as it has always worked.
	function setAutoCopies(v: boolean) {
		flow.updateNodeData(id, { autoCopies: v });
	}

	// The editor embeds a native plugin view, which needs the running audio
	// instance; there is nothing to open before the pipeline starts.
	// What the engine is actually running, which lags the node's own selection
	// while the graph rebuilds. Everything that touches the plugin waits on it:
	// acting sooner opens the outgoing plugin's editor instead of the new one.
	let status = $state<PluginStatus>({ path: null, hasEditor: true, copies: 0 });
	// A derived, not data.path: every updateNodeData hands the node a new data
	// object, which would rerun the status probe and flash the loading state.
	const pluginPath = $derived(data.path);
	const running = $derived(engine.isRunning);
	const loaded = $derived(!!pluginPath && status.path === pluginPath);
	const loading = $derived(!!data.path && engine.isRunning && !loaded);

	const canOpenEditor = $derived(loaded && engine.isRunning);

	$effect(() => {
		// Reading the path first both tracks it and clears the previous plugin's
		// verdict, so a stale answer never carries over to the new selection.
		const path = pluginPath;
		status = { path: null, hasEditor: true, copies: 0 };
		editorError = null;
		if (!path || !running) return;
		// A single probe would race the rebuild and read the outgoing plugin's
		// answer, which then never gets corrected; polling settles on the new
		// instance instead, and stops once it has.
		let timer: ReturnType<typeof setInterval> | undefined;
		const probe = () =>
			engine.audio
				.pluginStatus(id)
				.then((v) => {
					status = v;
					if (v.path === path) clearInterval(timer);
				})
				.catch(() => {});
		timer = setInterval(probe, 400);
		// Untracked: the probe reads engine state, which must not rerun this effect.
		untrack(probe);
		return () => clearInterval(timer);
	});

	// The copy count moves when the toggle flips or a wider cable is wired, long
	// after the status above has settled, so it is read on its own while the
	// plugin runs.
	$effect(() => {
		if (!loaded || !running) return;
		const timer = setInterval(() => {
			engine.audio
				.pluginStatus(id)
				.then((v) => {
					if (v.path === pluginPath) status = { ...status, copies: v.copies };
				})
				.catch(() => {});
		}, 1000);
		return () => clearInterval(timer);
	});

	// Copies need the same width in and out to line the channel groups up.
	const canCopy = $derived(
		!data.layout || (data.layout.inputs.find((b) => b.main)?.channels ?? 0) === (data.layout.outputs.find((b) => b.main)?.channels ?? -1)
	);

	// With one copy, only the plugin's own channels are processed; the rest of a
	// wider node goes through as it came.
	const processedChannels = $derived.by(() => {
		const width = data.layout?.inputs.find((b) => b.main)?.channels;
		if (!width) return undefined;
		if (status.copies > 1) return undefined;
		if (canCopy && (data.autoCopies ?? true)) return undefined;
		return width;
	});

	// Automatable parameters, editable directly in the node. Opt-in (most users
	// only need the editor). Readable only once the plugin is instantiated; a
	// poll keeps them in sync with edits made in the plugin's own window.
	let params = $state<PluginParam[]>([]);

	// When each parameter was last moved from this node. The plugin takes a write on
	// its next audio block, so a read just after one still shows the old value; that
	// is not an edit from the plugin's window.
	const touched = new Map<number, number>();
	const SETTLE_MS = 1500;

	async function loadParams() {
		const fresh = (await engine.audio.getPluginParams(id).catch(() => [])).filter((p) => !p.readOnly);
		// Same parameter set: update values in place so sliders aren't recreated
		// (and a mid-drag one isn't yanked). Otherwise swap the whole list.
		if (params.length === fresh.length && params.every((p, i) => p.id === fresh[i].id)) {
			// The plugin window drives the first copy only. A value that moved there is
			// handed to the others the way a knob on this node would be.
			const moved: Record<number, number> = {};
			const now = Date.now();
			let changed = false;
			for (let i = 0; i < fresh.length; i++) {
				if (fresh[i].value !== params[i].value) changed = true;
				if (status.copies > 1 && fresh[i].value !== params[i].value && now - (touched.get(fresh[i].id) ?? 0) > SETTLE_MS) {
					moved[fresh[i].id] = fresh[i].value;
				}
				params[i].value = fresh[i].value;
			}
			// A value that moved, here or in the plugin window, is kept in the project
			// right away; nothing is read while nothing changes.
			if (changed) saveSoon();
			if (Object.keys(moved).length) engine.audio.updateEffect(id, { pluginParams: moved }).catch(() => {});
		} else {
			params = fresh;
		}
	}

	const saving = $derived(data.saveParams ?? true);

	// A knob turn reaches the plugin on the next audio block, so the state is read a
	// moment later, once the drag has settled.
	let saveTimer: ReturnType<typeof setTimeout> | undefined;
	function saveSoon() {
		if (!saving) return;
		clearTimeout(saveTimer);
		saveTimer = setTimeout(captureState, 500);
	}

	function setParam(p: PluginParam, v: number) {
		p.value = v;
		touched.set(p.id, Date.now());
		engine.audio.updateEffect(id, { pluginParams: { [p.id]: v } }).catch(() => {});
		saveSoon();
	}

	function applyValues(values: Record<string, number>) {
		const now = Date.now();
		for (const p of params)
			if (values[p.id] !== undefined) {
				p.value = values[p.id];
				touched.set(p.id, now);
			}
		engine.audio.updateEffect(id, { pluginParams: values }).catch(() => {});
		saveSoon();
	}

	function setSaveParams(v: boolean) {
		// Off means the project stops keeping the settings: the saved ones go too,
		// so the plugin starts from its own defaults next time.
		flow.updateNodeData(id, v ? { saveParams: true } : { saveParams: false, state: null });
	}

	const pinnedIds = $derived(data.pinned ?? []);
	const pinned = $derived(pinnedIds.map((pid) => params.find((p) => p.id === pid)).filter((p): p is PluginParam => !!p));

	function togglePin(pid: number) {
		const next = pinnedIds.includes(pid) ? pinnedIds.filter((x) => x !== pid) : [...pinnedIds, pid];
		flow.updateNodeData(id, { pinned: next });
	}

	let query = $state('');
	const filtered = $derived(query.trim() ? params.filter((p) => p.name.toLowerCase().includes(query.trim().toLowerCase())) : params);

	function stepOf(p: PluginParam): number {
		return p.step || (p.max > p.min ? (p.max - p.min) / 100 : 0.01);
	}

	function formatValue(v: number): string {
		return Math.abs(v) >= 100 ? v.toFixed(0) : Math.abs(v) >= 10 ? v.toFixed(1) : v.toFixed(2);
	}

	$effect(() => {
		if (!loaded || !running) {
			params = [];
			return;
		}
		untrack(loadParams);
		const timer = setInterval(loadParams, 500);
		return () => clearInterval(timer);
	});

	// Pull the plugin's own state (edited via its GUI) into node data so it
	// survives project reload. State is non-structural, so this never rebuilds.
	async function captureState() {
		if (!saving) return;
		const capturedFor = data.path;
		const state = await engine.audio.getPluginState(id).catch(() => null);
		// The plugin may have been swapped while this was in flight. Writing now
		// would hand the previous plugin's blob to the new one, which then loads
		// it on the next rebuild and ends up in a state its own editor disagrees
		// with.
		if (data.path !== capturedFor) return;
		if (state && state !== data.state) flow.updateNodeData(id, { state });
	}

	// Last editor failure, shown in the node. The backend message names the
	// plugin and the step that failed, so it is worth surfacing verbatim.
	let editorError = $state<string | null>(null);

	function toggleEditor() {
		editorError = null;
		const next = !editorOpen;
		editorOpen = next;
		if (!next) captureState();
		const call = next ? engine.audio.openPluginEditor(id, data.name || 'Plugin') : engine.audio.closePluginEditor(id);
		call.catch((e: unknown) => {
			editorOpen = !next;
			editorError = String(e);
			// The probe only says the plugin advertises an editor; a failed open
			// is the stronger evidence, so the node stops offering the button.
			if (next) status = { ...status, hasEditor: false };
		});
	}

	$effect(() => {
		let unlisten: (() => void) | undefined;
		engine.audio
			.onPluginEditorClosed((closedId) => {
				if (closedId === id) {
					editorOpen = false;
					captureState();
				}
			})
			.then((fn) => (unlisten = fn));
		return () => unlisten?.();
	});

	// A stopped pipeline tears the editor down; reflect that in the button.
	$effect(() => {
		if (!engine.isRunning) editorOpen = false;
	});

	function toggleBypass() {
		const patch = { bypassed: !data.bypassed };
		flow.updateNodeData(id, patch);
		engine.audio.updateEffect(id, patch).catch(() => {});
	}
</script>

{#snippet badge()}
	{#if running && status.copies > 1}
		<Tooltip text={`${status.copies} copies of the plugin run, one per group of channels`}>
			<span class="rounded-md bg-(--lv-2) px-1.5 py-1 font-mono text-[10px] leading-none text-neutral-900 tabular-nums">x{status.copies}</span>
		</Tooltip>
	{/if}
{/snippet}

<Wrapper
	{badge}
	label={data.name || 'Plugin'}
	icon={Plug}
	accent="effect"
	hasInput
	hasOutput
	channelIo
	{processedChannels}
	inputs={extraInputs.length ? extraInputs : undefined}
	selfGrowing
	nodeId={id}
	bypassed={data.bypassed}
	onBypass={toggleBypass}>
	<div class="flex w-64 flex-col gap-2">
		{#if noPlugins}
			<div class="flex items-center justify-between gap-2 rounded-lg bg-(--field-bg) py-1 pr-1 pl-2.5">
				<span class="text-xs whitespace-nowrap text-neutral-900">No plugins installed</span>
				<Button class="nodrag nopan" size="xs" variant="ghost" disabled={engine.plugins.scanning} onclick={() => engine.plugins.rescan()}>
					<Refresh class={['size-3.5', engine.plugins.scanning && 'animate-spin']} />
					Rescan
				</Button>
			</div>
		{:else}
			<div class="flex items-center gap-1.5">
				<div class="min-w-0 flex-1">
					<Combobox
						class="nodrag nopan"
						{options}
						value={currentUid}
						placeholder={data.name || (engine.plugins.scanning ? 'Scanning...' : 'Choose a plugin')}
						emptyHint={engine.plugins.scanning ? 'Scanning...' : 'No plugins found'}
						onOpen={() => engine.plugins.refresh()}
						onChange={select}>
						{#snippet footer()}
							<RescanButton onRescan={() => engine.plugins.rescan()} />
						{/snippet}
					</Combobox>
				</div>
				{#if data.path}
					{#if canOpenEditor && status.hasEditor}
						<Tooltip text={editorOpen ? 'Close the plugin window' : 'Open the plugin window'}>
							<Button class="nodrag nopan" icon aria-label="Plugin window" tone={editorOpen ? 'accent' : 'neutral'} onclick={toggleEditor}>
								<WindowNew class="size-4" />
							</Button>
						</Tooltip>
					{:else}
						<Tooltip
							text={canOpenEditor
								? 'This plugin has no window of its own'
								: 'Start the pipeline to open the editor: the plugin loads only while it runs.'}>
							<Button class="nodrag nopan pointer-events-none" icon disabled aria-label="Plugin window">
								{#if canOpenEditor}<WindowOff class="size-4" />{:else}<WindowNew class="size-4" />{/if}
							</Button>
						</Tooltip>
					{/if}
				{/if}
			</div>
		{/if}

		{#if data.path}
			<Tooltip
				text={canCopy
					? 'A node wider than the plugin runs one copy of it per group of channels, so every channel is processed. Off: one copy, and the other channels pass through untouched.'
					: 'This plugin changes the channel count (its input and output differ), so it always runs as one copy.'}>
				<Toggle
					class="nodrag nopan"
					size="sm"
					label="Copy per channel group"
					disabled={!canCopy}
					checked={canCopy && (data.autoCopies ?? true)}
					onChange={setAutoCopies} />
			</Tooltip>
			<Tooltip
				text="Keep this plugin's settings in the project, so they come back after the pipeline stops or the project is reopened. Off: the plugin starts from its defaults.">
				<Toggle class="nodrag nopan" size="sm" label="Save settings" checked={saving} onChange={setSaveParams} />
			</Tooltip>
			{#if !data.layout && !probeError}
				<p class="text-xs text-neutral-800">Reading the plugin's ports...</p>
			{/if}
		{/if}

		{#if missing}
			<p class="text-xs text-red-500">Not installed, or failed to load</p>
		{/if}
		{#if engine.plugins.error}
			<p class="text-xs text-red-500">Plugin scan failed: {engine.plugins.error}</p>
		{/if}
		{#if probeError}
			<p class="text-xs text-red-500">Could not read the plugin's ports, so the node stays silent: {probeError}</p>
		{/if}
		{#if editorError}
			<p class="text-xs text-red-500">{editorError}</p>
		{/if}

		{#if data.path && !missing}
			{#if loading}
				<p class="flex items-center gap-1.5 text-xs text-neutral-900">
					<Refresh class="size-3.5 shrink-0 animate-spin" />
					Loading plugin...
				</p>
			{:else if !engine.isRunning}
				<p class="rounded-lg bg-(--field-bg) px-2.5 py-2 text-[11px] text-neutral-900">Run the pipeline to see and change the plugin's parameters.</p>
			{:else if params.length}
				<PluginPresets pluginId={data.pluginId} {params} onApply={applyValues} />

				{#if pinned.length}
					<div class="grid grid-cols-4 gap-x-1 gap-y-2 rounded-lg bg-(--field-bg) px-1 py-2 text-violet-500">
						{#each pinned as p (p.id)}
							<Knob
								class="nodrag nopan"
								label={p.name}
								value={p.value}
								min={p.min}
								max={p.max}
								step={p.step}
								defaultValue={p.default}
								format={formatValue}
								onChange={(v) => setParam(p, v)} />
						{/each}
					</div>
				{:else}
					<p class="rounded-lg border border-dashed border-theme/10 px-2.5 py-2 text-[11px] text-neutral-800">
						Pin parameters from the list below to get knobs here.
					</p>
				{/if}

				<div class="rounded-lg bg-(--field-bg) p-2.5">
					<button
						type="button"
						class="nodrag nopan flex w-full items-center gap-1 rounded-md py-0.5 text-[11px] text-neutral-900 hover:text-neutral-1100"
						onclick={() => flow.updateNodeData(id, { showParams: !data.showParams })}>
						<Chevron class={['size-3 transition-transform', data.showParams && 'rotate-90']} />
						All parameters
						<span class="text-neutral-700 tabular-nums">{params.length}</span>
					</button>

					{#if data.showParams}
						<div class="flex flex-col gap-1.5 pt-1.5" transition:slide={{ duration: 180 }}>
							<Input class="nodrag nopan" size="sm" placeholder="Search parameters" bind:value={query} />
							<ScrollArea class="nowheel nodrag" viewportClass="flex max-h-64 flex-col gap-1.5 pr-2">
								{#each filtered as p (p.id)}
									{@const isPinned = pinnedIds.includes(p.id)}
									<div class="flex items-end gap-1">
										<Slider
											class="nodrag nopan nowheel min-w-0 flex-1"
											label={p.name}
											value={p.value}
											min={p.min}
											max={p.max}
											step={stepOf(p)}
											defaultValue={p.default}
											onChange={(v) => setParam(p, v)} />
										<Button
											class="nodrag nopan"
											size="xs"
											icon
											variant="ghost"
											tone={isPinned ? 'accent' : 'neutral'}
											aria-label={isPinned ? 'Unpin' : 'Pin as knob'}
											title={isPinned ? 'Unpin' : 'Pin as knob'}
											onclick={() => togglePin(p.id)}>
											{#if isPinned}<PinOn class="size-3" />{:else}<PinOff class="size-3" />{/if}
										</Button>
									</div>
								{:else}
									<p class="py-2 text-center text-[11px] text-neutral-800">Nothing matches "{query}"</p>
								{/each}
							</ScrollArea>
						</div>
					{/if}
				</div>
			{/if}
		{/if}

		{#if running && status.copies > 1}
			<p class="text-[11px] leading-snug text-neutral-800">Changes made in the plugin window reach the other copies with a short delay.</p>
		{/if}
	</div>
</Wrapper>
