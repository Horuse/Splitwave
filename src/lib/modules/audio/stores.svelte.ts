import type { UnlistenFn } from '@tauri-apps/api/event';
import { methods } from './methods';
import type {
	AudioApplication,
	AudioDevice,
	StartPipelinePayload
} from './types';

class AudioStore {
	inputDevices = $state<AudioDevice[]>([]);
	outputDevices = $state<AudioDevice[]>([]);
	audioApplications = $state<AudioApplication[]>([]);
	isRunning = $state(false);
	runningPipelineId = $state<string | null>(null);
	startedAt = $state<number | null>(null);
	lastError = $state<string | null>(null);
	chooseFileNodeId = $state<string | null>(null);
	pendingRetryPipelineId = $state<string | null>(null);

	private unlisten: UnlistenFn | undefined;

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
				this.audioApplications = this.audioApplications.map((a) =>
					icons[a.bundleId] ? { ...a, icon: icons[a.bundleId] } : a
				);
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
				this.lastError = null;
			} else if (e.kind === 'stopped') {
				this.isRunning = false;
				this.runningPipelineId = null;
				this.startedAt = null;
			} else if (e.kind === 'error') {
				this.isRunning = false;
				this.runningPipelineId = null;
				this.startedAt = null;
				this.lastError = e.message;
			}
		});
	}

	async activatePipeline(pipelineId: string, graph: StartPipelinePayload): Promise<void> {
		try {
			await methods.startPipeline(graph);
		} catch (e) {
			if (this.routeStartError(e, pipelineId)) return;
			throw e;
		}
		this.runningPipelineId = pipelineId;
	}

	/** Apply a new graph to the running pipeline. Uses `reconcile_pipeline`,
	 * which diffs the new graph and only touches what changed — input
	 * streams stay alive across edits when their spec is unchanged.
	 * Falls back to stop + start if the pipeline isn't running. */
	async restartPipeline(graph: StartPipelinePayload): Promise<void> {
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

	private routeStartError(e: unknown, pipelineId?: string): boolean {
		const msg = e instanceof Error ? e.message : String(e);
		const m = /choose-file \(node ([^)]+)\)/.exec(msg);
		if (!m) return false;
		this.chooseFileNodeId = m[1];
		this.pendingRetryPipelineId = pipelineId ?? null;
		return true;
	}

	destroy(): void {
		this.unlisten?.();
		this.unlisten = undefined;
	}
}

export const audioStore = new AudioStore();
