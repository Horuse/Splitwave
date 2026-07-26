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
	import { appSettings } from '$lib/modules/settings/stores.svelte';
	import { methods as pipelineMethods } from '$lib/modules/pipeline/methods';
	import { audioStore } from '$lib/modules/audio/stores.svelte';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import {
		DND_MIME,
		defaultDataFor,
		emitNodeAction,
		freeRunFrom,
		fromXyEdges,
		fromXyNodes,
		nodeTypes,
		parseHandle,
		registry,
		toXyEdges,
		toXyNodes
	} from '../utils';
	import Sidebar from './sidebar.svelte';
	import ChannelEdge from './_channel_edge.svelte';
	import ConnectionLine from './_connection_line.svelte';
	import EdgeRectSelect from './_edge_rect_select.svelte';
	import OrphanEdges from './_orphan_edges.svelte';
	const edgeTypes = { channel: ChannelEdge };
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
	import { channelCaps, channelSelection } from '../stores.svelte';
	import { edgeSettings } from '../edge_settings.svelte';
	import { Menu, MenuItem as OverlayMenuItem } from '$lib/modules/overlay/ui';
	import type { Component } from 'svelte';

	let { pipeline }: { pipeline: Pipeline } = $props();

	const flow = useSvelteFlow();

	function sanitizeEdges(xyNodes: XyNode[], xyEdges: XyEdge[]): XyEdge[] {
		const ids = new Set(xyNodes.map((n) => n.id));
		return xyEdges.filter((e) => ids.has(e.source) && ids.has(e.target));
	}

	let nodes = $state.raw<XyNode[]>(untrack(() => toXyNodes(pipeline.nodes)));
	let edges = $state.raw<XyEdge[]>(untrack(() => {
		const n = toXyNodes(pipeline.nodes);
		return sanitizeEdges(n, toXyEdges(pipeline.edges));
	}));

	// Fresh edges only: an already-wired armed channel would re-fire the fan-out.
	// Not `onbeforeconnect` -- xyflow 1.5.2 throws on pointer-up when it is set.
	let seenEdgeIds = new Set<string>(untrack(() => edges.map((e) => e.id)));
	$effect(() => {
		const current = edges;
		const armed = channelSelection.channels;
		const from = channelSelection.nodeId;

		untrack(() => {
			const fresh = current.filter((e) => !seenEdgeIds.has(e.id));
			seenEdgeIds = new Set(current.map((e) => e.id));
			if (!from || armed.length < 2 || fresh.length === 0) return;

			const seed = fresh.find((e) => {
				const ch = e.sourceHandle ? parseHandle(e.sourceHandle) : null;
				return e.source === from && ch !== null && armed.includes(ch);
			});
			const seedCh = seed?.sourceHandle ? parseHandle(seed.sourceHandle) : null;
			if (!seed?.targetHandle || seedCh === null) return;

			const dropped = parseHandle(seed.targetHandle) ?? 1;
			const taken = current
				.filter((e) => e.target === seed.target && e.id !== seed.id)
				.map((e) => e.targetHandle ?? '');
			// The target may refuse the whole set: mono recording takes one channel.
			const cap = channelCaps.get(seed.target) ?? Infinity;
			const run = freeRunFrom(taken, dropped, armed.length).filter((ch) => ch <= cap);
			if (run.length === 0) return;
			const seedIdx = armed.indexOf(seedCh);

			const added: XyEdge[] = [];
			const retargeted = current.map((e) =>
				e.id === seed.id ? { ...e, targetHandle: `ch${run[seedIdx]}` } : e
			);
			armed.forEach((ch, i) => {
				if (i === seedIdx || run[i] === undefined) return;
				added.push({
					id: createId(),
					source: seed.source,
					sourceHandle: `ch${ch}`,
					target: seed.target,
					targetHandle: `ch${run[i]}`,
					animated: edgeSettings.animated,
					type: 'channel'
				});
			});
			edges = [...retargeted, ...added];
			added.forEach((e) => seenEdgeIds.add(e.id));
			channelSelection.clear();
		});
	});

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

	function copyNodes(group: XyNode[]) {
		if (group.length === 0) return;
		const originX = Math.min(...group.map((n) => n.position.x));
		const originY = Math.min(...group.map((n) => n.position.y));
		const index = new Map(group.map((n, i) => [n.id, i]));
		pipelineStore.clipboard = {
			origin: { x: originX, y: originY },
			nodes: group.map((n) => ({
				kind: n.type as NodeKind,
				data: JSON.parse(JSON.stringify(n.data)),
				dx: n.position.x - originX,
				dy: n.position.y - originY
			})),
			edges: edges.flatMap((e) => {
				const source = index.get(e.source);
				const target = index.get(e.target);
				if (source === undefined || target === undefined) return [];
				return [
					{
						source,
						target,
						sourceHandle: e.sourceHandle ?? undefined,
						targetHandle: e.targetHandle ?? undefined
					}
				];
			})
		};
	}

	function copyNode(node: XyNode) {
		copyNodes([node]);
	}

	function copySelection() {
		copyNodes(nodes.filter((n) => n.selected));
	}

	function selectAllNodes() {
		nodes = nodes.map((n) => (n.selected ? n : { ...n, selected: true }));
	}

	/** Diagonal nudge so a pasted group stays next to its source instead of on top. */
	const PASTE_OFFSET = 50;

	function pasteAt(position?: { x: number; y: number }) {
		const c = pipelineStore.clipboard;
		if (!c || c.nodes.length === 0) return;
		const origin = position ?? { x: c.origin.x + PASTE_OFFSET, y: c.origin.y + PASTE_OFFSET };
		const ids = c.nodes.map(() => createId());
		nodes = [
			...nodes.map((n) => (n.selected ? { ...n, selected: false } : n)),
			...c.nodes.map((n, i) => ({
				id: ids[i],
				type: n.kind,
				position: { x: origin.x + n.dx, y: origin.y + n.dy },
				data: JSON.parse(JSON.stringify(n.data)),
				selected: true
			}))
		];
		edges = [
			...edges,
			...c.edges.map((e) => ({
				id: createId(),
				type: 'channel',
				source: ids[e.source],
				sourceHandle: e.sourceHandle,
				target: ids[e.target],
				targetHandle: e.targetHandle
			}))
		];
		pipelineStore.clipboard = { ...c, origin };
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
			case 'noiseSuppressor':
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
			action: () => copyNodes(node.selected ? nodes.filter((n) => n.selected) : [node])
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
		const n = toXyNodes(p.nodes);
		nodes = n;
		edges = sanitizeEdges(n, toXyEdges(p.edges));
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
		copySelection,
		paste: () => pasteAt(undefined),
		selectAll: selectAllNodes,
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

	// Every node's data (minus canvas geometry) and every edge with its handles.
	// The backend reconcile classifies the resend, so a live-param-only change is
	// a no-op there and needs no field-by-field gate here.
	function routingSignature(): string {
		return JSON.stringify({
			nodes: nodes.map((n) => ({ id: n.id, type: n.type, data: n.data })),
			edges: edges.map((e) => ({
				id: e.id,
				source: e.source,
				sourceHandle: e.sourceHandle ?? null,
				target: e.target,
				targetHandle: e.targetHandle ?? null
			}))
		});
	}

	let lastRoutingSig = untrack(routingSignature);
	let restartTimer: ReturnType<typeof setTimeout> | undefined;
	// No teardown on re-run: node measurement re-fires this effect constantly and
	// would cancel the pending reconcile before it ever reaches the backend.
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
					audioStore.reportError(e);
				}
			});
		}, 400);
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

		if (e.key === 'Escape' && channelSelection.nodeId) {
			channelSelection.clear();
			return;
		}

		if (e.key === 'Backspace' || e.key === 'Delete') {
			if (inField) return;
			e.preventDefault();
			return;
		}

		const mod = e.metaKey || e.ctrlKey;
		if (!mod || inField) return;

		if (e.key === 'c' || e.key === 'C') {
			if (!nodes.some((n) => n.selected)) return;
			e.preventDefault();
			copySelection();
		} else if (e.key === 'v' || e.key === 'V') {
			if (!pipelineStore.clipboard) return;
			e.preventDefault();
			pasteAt(undefined);
		} else if (e.key === 'a' || e.key === 'A') {
			e.preventDefault();
			selectAllNodes();
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
		class="relative flex-1"
		role="region"
		aria-label="Flow editor"
		ondragover={onDragOver}
		ondrop={onDrop}
	>
		<SvelteFlow
			proOptions={{ hideAttribution: true }}
			class="!bg-background"
			bind:nodes
			bind:edges
			{nodeTypes}
			{edgeTypes}
			defaultEdgeOptions={{ animated: edgeSettings.animated, type: 'channel' }}
			connectionLineComponent={ConnectionLine}
			deleteKey={['Delete', 'Backspace']}
			onnodecontextmenu={onNodeContextMenu}
			onedgecontextmenu={onEdgeContextMenu}
			onpanecontextmenu={onPaneContextMenu}
			onpaneclick={closeContextMenu}
			onnodedragstart={closeContextMenu}
			onselectionstart={closeContextMenu}
			onmovestart={closeContextMenu}
			snapGrid={appSettings.snapToGrid ? [appSettings.gridSize, appSettings.gridSize] : undefined}
			fitView
		>
			<Background patternClass="fill-neutral-200"/>
			<Controls />
			<EdgeRectSelect />
			<OrphanEdges onDelete={deleteEdge} />
		</SvelteFlow>

		{#if channelSelection.channels.length > 0}
			<div
				class="pointer-events-none absolute bottom-4 left-1/2 z-10 -translate-x-1/2 rounded-full border border-neutral-400 bg-neutral-100 px-3 py-1.5 text-[11px] text-neutral-1000 shadow-sm"
			>
				<span class="font-mono tabular-nums">{channelSelection.channels.length}</span>
				channels armed &mdash; drag any one to connect them all, Esc to clear
			</div>
		{/if}
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
