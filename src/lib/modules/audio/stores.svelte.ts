import type { UnlistenFn } from '@tauri-apps/api/event';
import toast from 'svelte-french-toast';
import { open, save } from '@tauri-apps/plugin-dialog';
import { methods } from './methods';
import type { AudioApplication, AudioDevice, StartPipelinePayload } from './types';
import { methods as pipelineMethods } from '$lib/modules/pipeline/methods';
import { pipelineStore } from '$lib/modules/pipeline/stores.svelte';
import { isFromFuture } from '$lib/modules/pipeline/migrations';
import type { FileRecordingNodeData, PipelineNode, RecordingFormat } from '$lib/modules/pipeline/types';

// Mirrors `extension()` in the File Recording node: the dialog filter must
// match the encoder the node will actually write.
function recordingExtension(fmt: RecordingFormat): string {
	if (fmt.kind === 'flac') return 'flac';
	if (fmt.kind === 'opus') return 'opus';
	if (fmt.kind === 'mp3') return 'mp3';
	if (fmt.kind === 'aac') return 'm4a';
	if (fmt.kind === 'aiff') return 'aiff';
	return 'wav';
}

function pruneDanglingEdgesPayload(payload: StartPipelinePayload): StartPipelinePayload {
	const ids = new Set(payload.nodes.map((n) => n.id));
	const edges = payload.edges.filter((e) => ids.has(e.source) && ids.has(e.target));
	return edges.length === payload.edges.length ? payload : { ...payload, edges };
}

class AudioStore {
	inputDevices = $state<AudioDevice[]>([]);
	outputDevices = $state<AudioDevice[]>([]);
	audioApplications = $state<AudioApplication[]>([]);
	isRunning = $state(false);
	runningPipelineId = $state<string | null>(null);
	startedAt = $state<number | null>(null);
	chooseFileNodeId = $state<string | null>(null);
	pendingRetryPipelineId = $state<string | null>(null);
	pendingNodeIds = $state<Set<string>>(new Set());
	missingFilePaths = $state<Set<string>>(new Set());

	private lastGraph: StartPipelinePayload | null = null;
	private fullGraph: StartPipelinePayload | null = null;
	private reconnectTimer: ReturnType<typeof setInterval> | undefined;
	private speakerRecovering = false;
	private unlisten: UnlistenFn | undefined;
	private unlistenSpeakerError: UnlistenFn | undefined;

	async refreshInputDevices(): Promise<void> {
		this.inputDevices = await methods.listInputDevices();
	}

	async refreshOutputDevices(): Promise<void> {
		this.outputDevices = await methods.listOutputDevices();
	}

	async refreshAudioApplications(): Promise<void> {
		const apps = await methods.listAudioApplications().catch(() => [] as AudioApplication[]);
		this.audioApplications = apps;
		if (apps.length === 0) return;
		methods
			.getAppIcons(apps.map((a) => a.bundleId))
			.then((icons) => {
				this.audioApplications = this.audioApplications.map((a) => (icons[a.bundleId] ? { ...a, icon: icons[a.bundleId] } : a));
			})
			.catch(() => {});
	}

	async init(): Promise<void> {
		await Promise.all([this.refreshInputDevices(), this.refreshOutputDevices()]);
		void this.refreshAudioApplications();
		this.isRunning = await methods.isPipelineRunning().catch(() => false);
		if (this.isRunning) this.startedAt = Date.now();
		this.unlisten = await methods.onState((e) => {
			if (e.kind === 'started') {
				this.isRunning = true;
				this.startedAt = Date.now();
			} else if (e.kind === 'stopped') {
				this.stopPendingReconnectLoop();
				this.isRunning = false;
				this.runningPipelineId = null;
				this.startedAt = null;
			} else if (e.kind === 'error') {
				this.stopPendingReconnectLoop();
				this.isRunning = false;
				this.runningPipelineId = null;
				this.startedAt = null;
				this.reportError(e.message);
			}
		});
		methods
			.onSpeakerError(() => {
				if (this.speakerRecovering || !this.lastGraph || !this.isRunning) return;
				this.speakerRecovering = true;
				methods
					.reconcilePipeline(this.lastGraph)
					.catch((e: unknown) => {
						const msg = e instanceof Error ? e.message : String(e);
						if (!msg.includes('not running')) {
							this.isRunning = false;
							this.runningPipelineId = null;
							this.startedAt = null;
							this.reportError(msg);
						}
					})
					.finally(() => {
						this.speakerRecovering = false;
					});
			})
			.then((fn) => {
				this.unlistenSpeakerError = fn;
			})
			.catch(() => {});
	}

	async activatePipeline(pipelineId: string, graph: StartPipelinePayload): Promise<void> {
		this.lastGraph = graph;
		try {
			await methods.startPipeline(graph);
		} catch (e) {
			if (await this.handleStartError(e, pipelineId, graph)) return;
			throw e;
		}
		this.runningPipelineId = pipelineId;
		await pipelineMethods.setActivePipelineId(pipelineId).catch(() => {});
	}

	/** Routes a `choose-file` error. With the editor open its File Recording
	 * node owns the dialog (and writes the path back into the editor state);
	 * elsewhere the store opens the dialog and persists the choice itself. */
	private async handleStartError(e: unknown, pipelineId: string, graph: StartPipelinePayload): Promise<boolean> {
		const nodeId = this.chooseFileNodeIdFrom(e);
		if (!nodeId) return false;
		if (pipelineStore.editorActions) {
			this.chooseFileNodeId = nodeId;
			this.pendingRetryPipelineId = pipelineId;
			return true;
		}
		const next = await this.promptRecordingFile(pipelineId, graph, nodeId);
		if (!next) return true;
		await this.activatePipeline(pipelineId, next);
		return true;
	}

	private chooseFileNodeIdFrom(e: unknown): string | null {
		const msg = e instanceof Error ? e.message : String(e);
		const m = /choose-file \(node ([^)]+)\)/.exec(msg);
		return m ? m[1] : null;
	}

	/** Opens the save dialog for a recording node and returns the graph with the
	 * chosen path applied, or `null` when the user cancels. Persists the path so
	 * a later activation from the list won't prompt again. Append mode picks an
	 * existing file instead of asking for a new one. */
	private async promptRecordingFile(pipelineId: string, graph: StartPipelinePayload, nodeId: string): Promise<StartPipelinePayload | null> {
		const node = graph.nodes.find((n) => n.id === nodeId);
		if (!node) return null;
		const data = node.data as FileRecordingNodeData;
		const ext = recordingExtension(data.format);
		const append = data.mode === 'append';
		let path: string | null;
		if (append) {
			const picked = await open({
				title: 'Choose recording to append to',
				multiple: false,
				filters: [{ name: ext.toUpperCase(), extensions: [ext] }]
			});
			path = typeof picked === 'string' ? picked : null;
		} else {
			path = await save({ title: 'Save recording', filters: [{ name: ext.toUpperCase(), extensions: [ext] }] });
		}
		if (!path) return null;
		// A fresh path can be written unconditionally; only append keeps the
		// "extend this file" intent, everything else lands as a plain overwrite.
		const mode = append ? 'append' : 'overwrite';
		const patchNode = (n: PipelineNode): PipelineNode => (n.id === nodeId ? { ...n, data: { ...n.data, filePath: path, mode } } : n);
		void pipelineMethods
			.get(pipelineId)
			.then((p) => {
				if (!p) return;
				const nodes = p.nodes.map(patchNode);
				return pipelineMethods.save({ ...p, nodes, updatedAt: Date.now() });
			})
			.catch(() => {});
		return { nodes: graph.nodes.map(patchNode), edges: graph.edges };
	}

	/** Explicit user stop: unlike an engine-reported `stopped`/`error` event
	 * (handled in `init()`), this also forgets the persisted active pipeline
	 * so a future launch doesn't try to auto-activate it again. */
	async deactivatePipeline(): Promise<void> {
		this.stopPendingReconnectLoop();
		await methods.stopPipeline();
		await pipelineMethods.setActivePipelineId(null).catch(() => {});
	}

	/** Restores the pipeline that was active when the app last closed (or the
	 * PC last rebooted). Any App Audio / Audio File source not available yet
	 * is left out of the initial start and reconnected later by the polling
	 * loop once it becomes available. */
	async autoActivateOnLaunch(): Promise<void> {
		const id = await pipelineMethods.getActivePipelineId().catch(() => null);
		if (!id) return;
		const p = await pipelineMethods.get(id).catch(() => null);
		if (!p || isFromFuture(p)) return;
		const full: StartPipelinePayload = { nodes: p.nodes, edges: p.edges };
		this.fullGraph = full;
		const excluded = await this.unresolvedInputIds(full);
		this.pendingNodeIds = excluded;
		const reduced = this.buildReducedGraph(full, excluded);
		try {
			await methods.startPipeline(reduced);
		} catch (e) {
			this.reportError(e);
			return;
		}
		this.lastGraph = reduced;
		this.runningPipelineId = id;
		this.startPendingReconnectLoop();
	}

	/** Which input nodes in `full` can't resolve right now: an App Audio node
	 * whose bundle isn't currently running, or an Audio File node whose path
	 * doesn't exist on disk. Building this list also refreshes the app list,
	 * since a stale snapshot would wrongly exclude/include nodes. */
	private async unresolvedInputIds(full: StartPipelinePayload): Promise<Set<string>> {
		await this.refreshAudioApplications();
		const running = new Set(this.audioApplications.map((a) => a.bundleId));
		const unresolved = new Set<string>();
		const filePaths = new Set<string>();
		for (const n of full.nodes) {
			if (n.kind === 'appAudio') {
				const bundleId = (n.data as { bundleId: string | null }).bundleId;
				if (bundleId && !running.has(bundleId)) unresolved.add(n.id);
			} else if (n.kind === 'audioFile') {
				const filePath = (n.data as { filePath: string | null }).filePath;
				if (filePath) filePaths.add(filePath);
			}
		}
		const stillMissing = new Set<string>();
		await Promise.all(
			[...filePaths].map(async (path) => {
				const exists = await methods.pathExists(path).catch(() => true);
				if (!exists) stillMissing.add(path);
			})
		);
		this.missingFilePaths = stillMissing;
		for (const n of full.nodes) {
			if (n.kind === 'audioFile') {
				const filePath = (n.data as { filePath: string | null }).filePath;
				if (filePath && stillMissing.has(filePath)) unresolved.add(n.id);
			}
		}
		return unresolved;
	}

	private buildReducedGraph(full: StartPipelinePayload, excludeIds: Set<string>): StartPipelinePayload {
		if (excludeIds.size === 0) return full;
		const nodes: PipelineNode[] = full.nodes.filter((n) => !excludeIds.has(n.id));
		return pruneDanglingEdgesPayload({ nodes, edges: full.edges });
	}

	private startPendingReconnectLoop(): void {
		this.stopPendingReconnectLoop();
		this.reconnectTimer = setInterval(() => {
			void this.tryReconnectPending();
		}, 3000);
	}

	private stopPendingReconnectLoop(): void {
		if (this.reconnectTimer !== undefined) {
			clearInterval(this.reconnectTimer);
			this.reconnectTimer = undefined;
		}
	}

	private async tryReconnectPending(): Promise<void> {
		if (this.pendingNodeIds.size === 0 || !this.fullGraph || !this.isRunning) {
			this.stopPendingReconnectLoop();
			return;
		}
		const stillUnresolved = await this.unresolvedInputIds(this.fullGraph);
		if (stillUnresolved.size === this.pendingNodeIds.size) return;
		this.pendingNodeIds = stillUnresolved;
		const reduced = this.buildReducedGraph(this.fullGraph, stillUnresolved);
		try {
			await this.restartPipeline(reduced);
		} catch {
			return;
		}
		if (stillUnresolved.size === 0) this.stopPendingReconnectLoop();
	}

	/** Apply a new graph to the running pipeline. Uses `reconcile_pipeline`,
	 * which diffs the new graph and only touches what changed — input
	 * streams stay alive across edits when their spec is unchanged.
	 * Falls back to stop + start if the pipeline isn't running. */
	async restartPipeline(graph: StartPipelinePayload): Promise<void> {
		this.lastGraph = graph;
		let reconcileErr: unknown;
		try {
			await methods.reconcilePipeline(graph);
			return;
		} catch (e) {
			reconcileErr = e;
		}
		const msg = reconcileErr instanceof Error ? reconcileErr.message : String(reconcileErr);
		if (msg.includes('not running')) {
			try {
				await methods.startPipeline(graph);
			} catch (e) {
				if (this.routeStartError(e)) return;
				throw e;
			}
		} else {
			if (this.routeStartError(reconcileErr)) return;
			throw reconcileErr;
		}
	}

	private routeStartError(e: unknown): boolean {
		const nodeId = this.chooseFileNodeIdFrom(e);
		if (!nodeId) return false;
		this.chooseFileNodeId = nodeId;
		this.pendingRetryPipelineId = null;
		return true;
	}

	reportError(e: unknown): void {
		toast.error(e instanceof Error ? e.message : String(e));
	}

	destroy(): void {
		this.unlisten?.();
		this.unlisten = undefined;
		this.unlistenSpeakerError?.();
		this.unlistenSpeakerError = undefined;
	}
}

export const audioStore = new AudioStore();
