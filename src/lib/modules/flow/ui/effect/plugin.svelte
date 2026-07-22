<script lang="ts">
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { PluginNodeData } from '$lib/modules/pipeline/types';
	import type { PluginDescriptor } from '$lib/modules/audio/types';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import { audioStore } from '$lib/modules/audio/stores.svelte';
	import { Combobox, RescanButton } from '$lib/modules/form/ui';
	import Wrapper from '../node.svelte';
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
			vendor: desc?.vendor ?? ''
		});
	}

	let editorOpen = $state(false);

	// The editor embeds a native plugin view, which needs the running audio
	// instance; there is nothing to open before the pipeline starts.
	const canOpenEditor = $derived(!!data.path && audioStore.isRunning);

	function toggleEditor() {
		const next = !editorOpen;
		editorOpen = next;
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
				if (closedId === id) editorOpen = false;
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
		{/if}
	</div>
</Wrapper>
