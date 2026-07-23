<script lang="ts">
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { PluginNodeData } from '$lib/modules/pipeline/types';
	import type { PluginDescriptor, PluginParam } from '$lib/modules/audio/types';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import { audioStore } from '$lib/modules/audio/stores.svelte';
	import { Combobox, RescanButton } from '$lib/modules/form/ui';
	import Wrapper from '../node.svelte';
	import Slider from './_slider.svelte';
	import Toggle from '$lib/components/toggle.svelte';
	import { Plug } from '$lib/components/icons';

	type PluginNodeType = Node<PluginNodeData, 'plugin'>;
	let { id, data }: NodeProps<PluginNodeType> = $props();

	const flow = useSvelteFlow();

	let plugins = $state<PluginDescriptor[]>([]);
	let scanning = $state(false);
	let scanned = $state(false);

	// uid is the stable scan key; node data stores path + pluginId.
	const currentUid = $derived(
		plugins.find((p) => p.path === data.path && p.pluginId === data.pluginId)?.uid ?? null
	);

	const options = $derived(
		plugins.map((p) => ({
			value: p.uid,
			label: p.name,
			subtitle: p.vendor || null,
			badge: p.format
		}))
	);

	async function scan() {
		if (scanning) return;
		scanning = true;
		try {
			plugins = await audioMethods.scanPlugins();
		} catch {
			plugins = [];
		} finally {
			scanning = false;
			scanned = true;
		}
	}

	function select(uid: string | null) {
		const desc = plugins.find((p) => p.uid === uid);
		flow.updateNodeData(id, {
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
	const canOpenEditor = $derived(!!data.path && audioStore.isRunning);

	// Automatable parameters, editable directly in the node. Opt-in (most users
	// only need the editor). Readable only once the plugin is instantiated; a
	// poll keeps them in sync with edits made in the plugin's own window.
	let params = $state<PluginParam[]>([]);

	async function loadParams() {
		const fresh = (await audioMethods.getPluginParams(id).catch(() => [])).filter(
			(p) => !p.readOnly
		);
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
		audioMethods.updateEffect(id, { pluginParams: { [p.id]: v } }).catch(() => {});
	}

	$effect(() => {
		if (!data.showParams || !data.path || !audioStore.isRunning) {
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
		const state = await audioMethods.getPluginState(id).catch(() => null);
		if (state && state !== data.state) flow.updateNodeData(id, { state });
	}

	function toggleEditor() {
		const next = !editorOpen;
		editorOpen = next;
		if (!next) captureState();
		const call = next
			? audioMethods.openPluginEditor(id, data.name || 'Plugin')
			: audioMethods.closePluginEditor(id);
		call.catch((e) => {
			console.error('plugin editor error:', e);
			editorOpen = !next;
		});
	}

	$effect(() => {
		let unlisten: (() => void) | undefined;
		audioMethods
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
		if (!audioStore.isRunning) editorOpen = false;
	});

	function toggleBypass() {
		const patch = { bypassed: !data.bypassed };
		flow.updateNodeData(id, patch);
		audioMethods.updateEffect(id, patch).catch(() => {});
	}

	$effect(() => {
		if (!scanned) scan();
	});
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
	onBypass={toggleBypass}
>
	<div class="flex w-56 flex-col gap-2">
		<Combobox
			options={options}
			value={currentUid}
			placeholder={scanning ? 'Scanning…' : 'Select a plugin'}
			emptyHint={scanned ? 'No plugins found' : 'Scanning…'}
			onChange={select}
		>
			{#snippet footer()}
				<RescanButton onRescan={scan} />
			{/snippet}
		</Combobox>
		{#if data.path}
			<button
				class="nodrag nopan rounded-lg border border-neutral-400 bg-neutral-100 px-3 py-1.5 text-sm font-medium hover:bg-neutral-200 disabled:cursor-not-allowed disabled:opacity-50"
				disabled={!canOpenEditor}
				title={canOpenEditor ? undefined : 'Start the pipeline to open the editor'}
				onclick={toggleEditor}
			>
				{editorOpen ? 'Close editor' : 'Open editor'}
			</button>
			<p class="truncate font-mono text-[10px] text-neutral-800" title={data.path}>
				{data.pluginId}
			</p>
			<Toggle
				size="sm"
				label="Show parameters"
				checked={!!data.showParams}
				onChange={(v) => flow.updateNodeData(id, { showParams: v })}
			/>
			{#if data.showParams && params.length}
				<div class="nowheel nodrag flex max-h-64 flex-col gap-1.5 overflow-y-auto pr-1">
					{#each params as p (p.id)}
						<Slider
							label={p.name}
							value={p.value}
							min={p.min}
							max={p.max}
							step={p.stepped ? 1 : p.max > p.min ? (p.max - p.min) / 100 : 0.01}
							defaultValue={p.default}
							onChange={(v) => setParam(p, v)}
						/>
					{/each}
				</div>
			{/if}
		{/if}
	</div>
</Wrapper>
