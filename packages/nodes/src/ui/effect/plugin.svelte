<script lang="ts">
	import { Combobox, RescanButton, Slider, Toggle } from '@splitwave/ui';
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { PluginNodeData } from '@splitwave/nodes';
	import Wrapper from '../node.svelte';
	import Plug from '~icons/fluent/plug-connected-16-filled';
	import Refresh from '~icons/fluent/arrow-clockwise-16-filled';
	import WindowOff from '~icons/fluent/window-multiple-off-16-regular';
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
		if (data.path) engine.plugins.ensure();
	});

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
			state: null
		});
	}

	let editorOpen = $state(false);

	// The editor embeds a native plugin view, which needs the running audio
	// instance; there is nothing to open before the pipeline starts.
	// What the engine is actually running, which lags the node's own selection
	// while the graph rebuilds. Everything that touches the plugin waits on it:
	// acting sooner opens the outgoing plugin's editor instead of the new one.
	let status = $state<PluginStatus>({ path: null, hasEditor: true });
	const loaded = $derived(!!data.path && status.path === data.path);
	const loading = $derived(!!data.path && engine.isRunning && !loaded);

	const canOpenEditor = $derived(loaded && engine.isRunning);

	$effect(() => {
		// Reading the path first both tracks it and clears the previous plugin's
		// verdict, so a stale answer never carries over to the new selection.
		const path = data.path;
		status = { path: null, hasEditor: true };
		editorError = null;
		if (!path || !engine.isRunning) return;
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
		probe();
		return () => clearInterval(timer);
	});

	// Automatable parameters, editable directly in the node. Opt-in (most users
	// only need the editor). Readable only once the plugin is instantiated; a
	// poll keeps them in sync with edits made in the plugin's own window.
	let params = $state<PluginParam[]>([]);

	async function loadParams() {
		const fresh = (await engine.audio.getPluginParams(id).catch(() => [])).filter((p) => !p.readOnly);
		// Same parameter set: update values in place so sliders aren't recreated
		// (and a mid-drag one isn't yanked). Otherwise swap the whole list.
		if (params.length === fresh.length && params.every((p, i) => p.id === fresh[i].id)) {
			for (let i = 0; i < fresh.length; i++) params[i].value = fresh[i].value;
		} else {
			params = fresh;
		}
	}

	function setParam(p: PluginParam, v: number) {
		p.value = v;
		engine.audio.updateEffect(id, { pluginParams: { [p.id]: v } }).catch(() => {});
	}

	$effect(() => {
		if (!data.showParams || !loaded || !engine.isRunning) {
			params = [];
			return;
		}
		loadParams();
		const timer = setInterval(loadParams, 500);
		return () => clearInterval(timer);
	});

	// Pull the plugin's own state (edited via its GUI) into node data so it
	// survives project reload. State is non-structural, so this never rebuilds.
	async function captureState() {
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

<Wrapper
	label={data.name || 'Plugin'}
	icon={Plug}
	accent="effect"
	hasInput
	hasOutput
	channelIo
	selfGrowing
	nodeId={id}
	bypassed={data.bypassed}
	onBypass={toggleBypass}>
	<div class="flex w-56 flex-col gap-2">
		<Combobox
			class="nodrag nopan"
			{options}
			value={currentUid}
			placeholder={data.name || (engine.plugins.scanning ? 'Scanning…' : 'Select a plugin')}
			emptyHint={engine.plugins.scanning ? 'Scanning…' : 'No plugins found'}
			onOpen={() => engine.plugins.refresh()}
			onChange={select}>
			{#snippet footer()}
				<RescanButton onRescan={() => engine.plugins.rescan()} />
			{/snippet}
		</Combobox>
		{#if missing}
			<p class="text-xs text-rose-400">Not installed, or failed to load</p>
		{/if}
		{#if engine.plugins.error}
			<p class="text-xs text-rose-400">Plugin scan failed: {engine.plugins.error}</p>
		{/if}
		{#if data.path}
			{#if loading}
				<p class="flex items-center gap-1.5 text-sm text-neutral-900">
					<Refresh class="h-4 w-4 shrink-0 animate-spin" />
					Loading plugin...
				</p>
			{:else if canOpenEditor && !status.hasEditor}
				<p class="flex items-center gap-1.5 text-sm text-neutral-900">
					<WindowOff class="h-4 w-4 shrink-0" />
					No editor
				</p>
				{#if editorError}
					<p class="text-xs text-rose-400">{editorError}</p>
				{/if}
			{:else}
				<button
					class="nodrag nopan rounded-lg border border-neutral-400 bg-neutral-100 px-3 py-1.5 text-sm font-medium hover:bg-neutral-200 disabled:cursor-not-allowed disabled:opacity-50"
					disabled={!canOpenEditor}
					title={canOpenEditor ? undefined : 'Start the pipeline to open the editor'}
					onclick={toggleEditor}>
					{editorOpen ? 'Close editor' : 'Open editor'}
				</button>
			{/if}
			<p class="truncate font-mono text-[10px] text-neutral-800" title={data.path}>
				{data.pluginId}
			</p>
			<Toggle
				class="nodrag nopan"
				size="sm"
				label="Show parameters"
				checked={!!data.showParams}
				onChange={(v) => flow.updateNodeData(id, { showParams: v })} />
			{#if data.showParams && params.length}
				<div class="nowheel nodrag flex max-h-64 flex-col gap-1.5 overflow-y-auto pr-1">
					{#each params as p (p.id)}
						<Slider
							class="nodrag nopan nowheel"
							label={p.name}
							value={p.value}
							min={p.min}
							max={p.max}
							step={p.step || (p.max > p.min ? (p.max - p.min) / 100 : 0.01)}
							defaultValue={p.default}
							onChange={(v) => setParam(p, v)} />
					{/each}
				</div>
			{/if}
		{/if}
	</div>
</Wrapper>
