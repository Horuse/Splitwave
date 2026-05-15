<script lang="ts">
	import {
		Background,
		Controls,
		SvelteFlow,
		useSvelteFlow,
		type Edge as XyEdge,
		type Node as XyNode
	} from '@xyflow/svelte';
	import { createId } from '@paralleldrive/cuid2';
	import { listen, type UnlistenFn } from '@tauri-apps/api/event';
	import { onDestroy, onMount, untrack } from 'svelte';
	import type { NodeKind, Pipeline } from '$lib/modules/pipeline/types';
	import { pipelineStore } from '$lib/modules/pipeline/stores.svelte';
	import { methods as pipelineMethods } from '$lib/modules/pipeline/methods';
	import { audioStore } from '$lib/modules/audio/stores.svelte';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import {
		DND_MIME,
		defaultDataFor,
		emitNodeAction,
		fromXyEdges,
		fromXyNodes,
		nodeTypes,
		registry,
		toXyEdges,
		toXyNodes
	} from '../utils';
	import Sidebar from './sidebar.svelte';
	import {
		Backspace,
		Copy,
		ClipboardPaste,
		Delete,
		Folder,
		KeyCommand,
		Loop,
		Refresh,
		Rewind
	} from '$lib/components/icons';
	import { Menu, MenuItem as OverlayMenuItem } from '$lib/modules/overlay/ui';
	import type { Component } from 'svelte';

	let { pipeline }: { pipeline: Pipeline } = $props();

	const flow = useSvelteFlow();

	let nodes = $state.raw<XyNode[]>(untrack(() => toXyNodes(pipeline.nodes)));
	let edges = $state.raw<XyEdge[]>(untrack(() => toXyEdges(pipeline.edges)));

	type MenuItem = {
		label: string;
		icon?: Component;
		shortcut?: string;
		danger?: boolean;
		disabled?: boolean;
		action: () => void;
	};

	type ContextMenu =
		| { kind: 'node'; nodeId: string; x: number; y: number; items: MenuItem[] }
		| { kind: 'edge'; edgeId: string; x: number; y: number; items: MenuItem[] }
		| { kind: 'pane'; x: number; y: number; flowX: number; flowY: number; items: MenuItem[] };

	let contextMenu = $state<ContextMenu | null>(null);

	function onDragOver(event: DragEvent) {
		event.preventDefault();
		if (event.dataTransfer) event.dataTransfer.dropEffect = 'move';
	}

	function onDrop(event: DragEvent) {
		event.preventDefault();
		const kind = event.dataTransfer?.getData(DND_MIME) as NodeKind | undefined;
		if (!kind || !(kind in registry)) return;
		const position = flow.screenToFlowPosition({ x: event.clientX, y: event.clientY });
		addNode(kind, position);
	}

	function addNode(kind: NodeKind, position?: { x: number; y: number }) {
		addNodeWithData(kind, defaultDataFor(kind), position);
	}

	function addNodeWithData(
		kind: NodeKind,
		data: Record<string, unknown>,
		position?: { x: number; y: number }
	) {
		const fallback = { x: 100 + nodes.length * 40, y: 100 + nodes.length * 40 };
		nodes = [
			...nodes,
			{
				id: createId(),
				type: kind,
				position: position ?? fallback,
				data
			}
		];
	}

	function deleteNode(nodeId: string) {
		nodes = nodes.filter((n) => n.id !== nodeId);
		edges = edges.filter((e) => e.source !== nodeId && e.target !== nodeId);
	}

	function deleteEdge(edgeId: string) {
		edges = edges.filter((e) => e.id !== edgeId);
	}

	function copyNode(node: XyNode) {
		pipelineStore.clipboard = {
			kind: node.type as NodeKind,
			data: JSON.parse(JSON.stringify(node.data))
		};
	}

	function pasteAt(position?: { x: number; y: number }) {
		const c = pipelineStore.clipboard;
		if (!c) return;
		addNodeWithData(c.kind, JSON.parse(JSON.stringify(c.data)), position);
	}

	function patchNodeData(nodeId: string, patch: Record<string, unknown>) {
		flow.updateNodeData(nodeId, patch);
	}

	function nodeMenuItems(node: XyNode): MenuItem[] {
		const items: MenuItem[] = [];
		const id = node.id;
		const data = (node.data ?? {}) as Record<string, unknown>;
		const kind = node.type as NodeKind | undefined;

		switch (kind) {
			case 'audioFile':
				items.push({
					label: 'Choose file...',
					icon: Folder,
					action: () => emitNodeAction(id, 'chooseFile')
				});
				items.push({
					label: 'Rewind',
					icon: Rewind,
					disabled: !audioStore.isRunning || !data.filePath,
					action: () => audioMethods.seekAudioFile(id, 0).catch(() => {})
				});
				items.push({
					label: data.loopEnabled ? 'Loop off' : 'Loop on',
					icon: Loop,
					action: () => patchNodeData(id, { loopEnabled: !data.loopEnabled })
				});
				break;
			case 'microphone':
			case 'appAudio':
			case 'speaker':
				items.push({
					label: 'Refresh',
					icon: Refresh,
					action: () => emitNodeAction(id, 'refresh')
				});
				break;
			case 'fileRecording':
				items.push({
					label: 'Choose file...',
					icon: Folder,
					action: () => emitNodeAction(id, 'chooseFile')
				});
				break;
			case 'mute':
				items.push({
					label: data.muted ? 'Unmute' : 'Mute',
					action: () => {
						const patch = { muted: !data.muted };
						patchNodeData(id, patch);
						audioMethods.updateEffect(id, patch).catch(() => {});
					}
				});
				break;
			case 'levelMeter':
				items.push({
					label: 'Reset peaks',
					action: () => emitNodeAction(id, 'resetPeaks')
				});
				break;
			case 'gain':
			case 'channelBalance':
			case 'saturator':
			case 'eq':
			case 'limiter':
			case 'compressor':
			case 'noiseGate':
			case 'delay':
			case 'reverb':
				items.push({
					label: data.bypassed ? 'Engage' : 'Bypass',
					action: () => {
						const patch = { bypassed: !data.bypassed };
						patchNodeData(id, patch);
						audioMethods.updateEffect(id, patch).catch(() => {});
					}
				});
				break;
		}

		items.push({
			label: 'Copy',
			icon: Copy,
			shortcut: '⌘C',
			action: () => copyNode(node)
		});
		items.push({
			label: 'Delete',
			icon: Delete,
			shortcut: '⌫',
			danger: true,
			action: () => deleteNode(id)
		});
		return items;
	}

	function paneMenuItems(flowX: number, flowY: number): MenuItem[] {
		return [
			{
				label: 'Paste',
				icon: ClipboardPaste,
				shortcut: '⌘V',
				disabled: pipelineStore.clipboard === null,
				action: () => pasteAt({ x: flowX, y: flowY })
			}
		];
	}

	function onNodeContextMenu({ node, event }: { node: XyNode; event: MouseEvent }) {
		event.preventDefault();
		contextMenu = {
			kind: 'node',
			nodeId: node.id,
			x: event.clientX,
			y: event.clientY,
			items: nodeMenuItems(node)
		};
	}

	function onEdgeContextMenu({ edge, event }: { edge: XyEdge; event: MouseEvent }) {
		event.preventDefault();
		contextMenu = {
			kind: 'edge',
			edgeId: edge.id,
			x: event.clientX,
			y: event.clientY,
			items: [
				{
					label: 'Delete',
					icon: Delete,
					shortcut: '⌫',
					danger: true,
					action: () => deleteEdge(edge.id)
				}
			]
		};
	}

	function onPaneContextMenu({ event }: { event: MouseEvent | TouchEvent }) {
		if (!(event instanceof MouseEvent)) return;
		event.preventDefault();
		const flowPos = flow.screenToFlowPosition({ x: event.clientX, y: event.clientY });
		contextMenu = {
			kind: 'pane',
			x: event.clientX,
			y: event.clientY,
			flowX: flowPos.x,
			flowY: flowPos.y,
			items: paneMenuItems(flowPos.x, flowPos.y)
		};
	}

	function closeContextMenu() {
		contextMenu = null;
	}

	function runMenuItem(item: MenuItem) {
		if (item.disabled) return;
		item.action();
		contextMenu = null;
	}

	function getSnapshot(): Pipeline {
		return {
			id: pipeline.id,
			name: pipeline.name,
			createdAt: pipeline.createdAt,
			nodes: fromXyNodes(nodes),
			edges: fromXyEdges(edges),
			updatedAt: Date.now()
		};
	}

	function revertToSnapshot(p: Pipeline) {
		nodes = toXyNodes(p.nodes);
		edges = toXyEdges(p.edges);
	}

	// Capture on the debounced save tick when enough time has passed --
	// piggy-backs on real edits, no blind interval.
	const SNAPSHOT_MIN_SPACING_MS = 30_000;
	let lastSnapshotSig = '';
	let lastSnapshotAt = 0;
	function snapshotSignature(p: Pipeline): string {
		return JSON.stringify({ nodes: p.nodes, edges: p.edges });
	}

	// Undo/redo history. Cursor points at the current state inside `history`;
	// undo decrements, redo increments. New edits truncate forward history.
	const MAX_HISTORY = 50;
	let history = $state.raw<Pipeline[]>([untrack(() => getSnapshot())]);
	let cursor = $state(0);
	let canUndo = $derived(cursor > 0);
	let canRedo = $derived(cursor < history.length - 1);

	function captureIfChanged(snap: Pipeline) {
		const sig = snapshotSignature(snap);
		const currentSig = snapshotSignature(history[cursor]);
		if (sig === currentSig) return;
		const next = history.slice(0, cursor + 1);
		next.push(snap);
		const trimmed = next.length > MAX_HISTORY ? next.slice(next.length - MAX_HISTORY) : next;
		history = trimmed;
		cursor = trimmed.length - 1;
	}

	function commit(snap: Pipeline) {
		pipelineStore.save(snap);
		const sig = snapshotSignature(snap);
		const now = Date.now();
		if (sig !== lastSnapshotSig && now - lastSnapshotAt >= SNAPSHOT_MIN_SPACING_MS) {
			pipelineMethods.addSnapshot(snap).then(() => {
				lastSnapshotSig = sig;
				lastSnapshotAt = now;
			});
		}
		captureIfChanged(snap);
	}

	function flushPendingCommit() {
		if (saveTimer === undefined) return;
		clearTimeout(saveTimer);
		saveTimer = undefined;
		untrack(() => commit(getSnapshot()));
	}

	function undo() {
		flushPendingCommit();
		if (cursor === 0) return;
		cursor -= 1;
		revertToSnapshot(history[cursor]);
	}

	function redo() {
		flushPendingCommit();
		if (cursor >= history.length - 1) return;
		cursor += 1;
		revertToSnapshot(history[cursor]);
	}

	pipelineStore.editorActions = {
		addNode,
		addNodeWithData,
		getSnapshot,
		revertToSnapshot,
		undo,
		redo,
		canUndo: () => canUndo,
		canRedo: () => canRedo
	};

	let saveTimer: ReturnType<typeof setTimeout> | undefined;
	$effect(() => {
		nodes;
		edges;
		clearTimeout(saveTimer);
		saveTimer = setTimeout(() => {
			saveTimer = undefined;
			untrack(() => commit(getSnapshot()));
		}, 500);
		return () => clearTimeout(saveTimer);
	});

	onMount(() => {
		lastSnapshotSig = snapshotSignature(getSnapshot());
		// First edit always snapshots -- pretend the previous capture was
		// just past the spacing window.
		lastSnapshotAt = Date.now() - SNAPSHOT_MIN_SPACING_MS - 1;
	});

	// Auto-restart on routing changes only — effect params flow through
	// update_effect live, no restart needed.
	function routingSignature(): string {
		return JSON.stringify({
			nodes: nodes.map((n) => ({
				id: n.id,
				type: n.type,
				deviceId: (n.data as Record<string, unknown>).deviceId ?? null,
				bundleId: (n.data as Record<string, unknown>).bundleId ?? null,
				filePath: (n.data as Record<string, unknown>).filePath ?? null,
				excludeCurrentApp: (n.data as Record<string, unknown>).excludeCurrentApp ?? null
			})),
			edges: edges.map((e) => ({
				id: e.id,
				source: e.source,
				target: e.target,
				targetHandle: e.targetHandle ?? null
			}))
		});
	}

	let lastRoutingSig = untrack(routingSignature);
	let restartTimer: ReturnType<typeof setTimeout> | undefined;
	$effect(() => {
		const sig = routingSignature();
		if (sig === lastRoutingSig) return;
		lastRoutingSig = sig;
		if (!audioStore.isRunning) return;
		clearTimeout(restartTimer);
		restartTimer = setTimeout(() => {
			untrack(async () => {
				try {
					await audioStore.restartPipeline({
						nodes: fromXyNodes(nodes),
						edges: fromXyEdges(edges)
					});
				} catch (e) {
					audioStore.lastError = e instanceof Error ? e.message : String(e);
				}
			});
		}, 400);
		return () => clearTimeout(restartTimer);
	});

	// The Tauri WebView (and historic browser behavior) treats Backspace outside
	// of editable fields as "navigate back". XYFlow also defaults `deleteKey` to
	// Backspace, so we explicitly accept Delete too and swallow the default
	// navigation in case the press lands outside the flow.
	function onWindowKeyDown(e: KeyboardEvent) {
		const t = e.target as HTMLElement | null;
		const tag = t?.tagName?.toLowerCase();
		const inField =
			tag === 'input' || tag === 'textarea' || tag === 'select' || t?.isContentEditable;

		if (e.key === 'Backspace' || e.key === 'Delete') {
			if (inField) return;
			e.preventDefault();
			return;
		}

		const mod = e.metaKey || e.ctrlKey;
		if (!mod || inField) return;

		if (e.key === 'c' || e.key === 'C') {
			const selected = nodes.find((n) => n.selected);
			if (!selected) return;
			e.preventDefault();
			copyNode(selected);
		} else if (e.key === 'v' || e.key === 'V') {
			if (!pipelineStore.clipboard) return;
			e.preventDefault();
			pasteAt(undefined);
		}
	}

	// Auto-stop the pipeline when every AudioFile source has reached EOF and
	// no live capture (mic / system / app) is running. Mixed graphs keep
	// running so live recording survives the file finishing.
	const LIVE_INPUT_TYPES = ['microphone', 'systemAudio', 'appAudio'];
	let audioFileDone = $state<Record<string, boolean>>({});

	$effect(() => {
		if (!audioStore.isRunning) {
			audioFileDone = {};
		}
	});

	interface AudioFileProgress {
		nodeId: string;
		stopped: boolean;
	}

	function onBeforeUnload() {
		flushPendingCommit();
	}

	let unlistenAudioFile: UnlistenFn | undefined;
	onMount(() => {
		window.addEventListener('keydown', onWindowKeyDown, { capture: true });
		window.addEventListener('beforeunload', onBeforeUnload);
		listen<AudioFileProgress>('audio://audio_file_progress', (e) => {
			const { nodeId, stopped } = e.payload;
			if (!audioStore.isRunning) return;
			audioFileDone[nodeId] = stopped;
			if (!stopped) return;
			const hasLive = nodes.some((n) => LIVE_INPUT_TYPES.includes(n.type ?? ''));
			if (hasLive) return;
			const audioFiles = nodes.filter((n) => n.type === 'audioFile');
			if (audioFiles.length === 0) return;
			if (audioFiles.every((n) => audioFileDone[n.id])) {
				audioMethods.stopPipeline().catch(() => {});
			}
		}).then((fn) => {
			unlistenAudioFile = fn;
		});
		return () => {
			window.removeEventListener('keydown', onWindowKeyDown, { capture: true });
			window.removeEventListener('beforeunload', onBeforeUnload);
		};
	});

	onDestroy(() => {
		flushPendingCommit();
		clearTimeout(restartTimer);
		unlistenAudioFile?.();
		if (pipelineStore.editorActions?.getSnapshot === getSnapshot) {
			pipelineStore.editorActions = null;
		}
	});
</script>

<svelte:window onmousedown={closeContextMenu} />

<div class="flex h-full w-full">
	<div
		class="flex-1"
		role="region"
		aria-label="Flow editor"
		ondragover={onDragOver}
		ondrop={onDrop}
	>
		<SvelteFlow
			class="!bg-background"
			bind:nodes
			bind:edges
			{nodeTypes}
			defaultEdgeOptions={{ animated: true }}
			deleteKey={['Delete', 'Backspace']}
			onnodecontextmenu={onNodeContextMenu}
			onedgecontextmenu={onEdgeContextMenu}
			onpanecontextmenu={onPaneContextMenu}
			onpaneclick={closeContextMenu}
			onnodedragstart={closeContextMenu}
			onselectionstart={closeContextMenu}
			onmovestart={closeContextMenu}
			fitView
		>
			<Background patternClass="fill-neutral-200"/>
			<Controls />
		</SvelteFlow>
	</div>
	<Sidebar />
</div>

{#if contextMenu}
	<div
		class="fixed z-50"
		style="top: {contextMenu.y}px; left: {contextMenu.x}px"
		oncontextmenu={(e) => e.preventDefault()}
		onmousedown={(e) => e.stopPropagation()}
	>
		<Menu>
			{#each contextMenu.items as item (item.label)}
				<OverlayMenuItem
					label={item.label}
					icon={item.icon}
					danger={item.danger}
					disabled={item.disabled}
					onclick={() => runMenuItem(item)}
				>
					{#snippet shortcut()}
						{#if item.shortcut}
							{#each [...item.shortcut] as ch, i (i)}
								{#if ch === '⌘'}
									<KeyCommand class="h-3 w-3" />
								{:else if ch === '⌫'}
									<Backspace class="h-3 w-3" />
								{:else}
									<span>{ch}</span>
								{/if}
							{/each}
						{/if}
					{/snippet}
				</OverlayMenuItem>
			{/each}
		</Menu>
	</div>
{/if}
