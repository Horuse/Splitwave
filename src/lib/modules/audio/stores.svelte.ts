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
	lastError = $state<string | null>(null);

	private unlisten: UnlistenFn | undefined;

	async refreshInputDevices(): Promise<void> {
		this.inputDevices = await methods.listInputDevices();
	}

	async refreshOutputDevices(): Promise<void> {
		this.outputDevices = await methods.listOutputDevices();
	}

	async refreshAudioApplications(): Promise<void> {
		// Can fail without ScreenCaptureKit access; treat as empty.
		this.audioApplications = await methods
			.listAudioApplications()
			.catch(() => [] as AudioApplication[]);
	}

	async init(): Promise<void> {
		// App icon generation is slow on first call; fill in the background.
		await Promise.all([this.refreshInputDevices(), this.refreshOutputDevices()]);
		void this.refreshAudioApplications();
		this.unlisten = await methods.onState((e) => {
			if (e.kind === 'started') {
				this.isRunning = true;
				this.lastError = null;
			} else if (e.kind === 'stopped') {
				this.isRunning = false;
			} else if (e.kind === 'error') {
				this.isRunning = false;
				this.lastError = e.message;
			}
		});
	}

	/** Apply a new graph to the running pipeline. Uses `reconcile_pipeline`,
	 * which diffs the new graph and only touches what changed — input
	 * streams stay alive across edits when their spec is unchanged.
	 * Falls back to stop + start if the pipeline isn't running. */
	async restartPipeline(graph: StartPipelinePayload): Promise<void> {
		try {
			await methods.reconcilePipeline(graph);
		} catch (e) {
			const msg = e instanceof Error ? e.message : String(e);
			if (msg.includes('not running')) {
				await methods.startPipeline(graph);
			} else {
				throw e;
			}
		}
	}

	destroy(): void {
		this.unlisten?.();
		this.unlisten = undefined;
	}
}

export const audioStore = new AudioStore();
