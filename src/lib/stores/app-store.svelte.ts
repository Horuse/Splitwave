import { getContext, setContext } from 'svelte';
import type { Pipeline } from '$lib/domain/pipeline';
import type { NodeKind } from '$lib/domain/audio-node';
import type { AudioDevice } from '$lib/domain/device';
import { PipelineRepository } from '$lib/services/pipeline-repository';
import { TauriAudioService } from '$lib/services/audio-service';

// Single rune-store for the demo. Splits when there's a reason to split.
export class AppStore {
	pipelines = $state<Pipeline[]>([]);
	inputDevices = $state<AudioDevice[]>([]);
	outputDevices = $state<AudioDevice[]>([]);
	isRunning = $state(false);
	lastError = $state<string | null>(null);
	// Editor exposes imperative actions here. NOT reactive on purpose: writing
	// to this from inside the editor's `$effect(nodes, edges)` would cascade
	// updates back into the xyflow reactive graph and trip `effect_update_depth_exceeded`.
	editorActions: {
		addNode: (kind: NodeKind) => void;
		getSnapshot: () => Pipeline | null;
	} | null = null;

	constructor(
		readonly repo: PipelineRepository,
		readonly audio: TauriAudioService
	) {}

	async refreshPipelines(): Promise<void> {
		this.pipelines = await this.repo.list();
	}

	async refreshDevices(): Promise<void> {
		const [ins, outs] = await Promise.all([
			this.audio.listInputDevices(),
			this.audio.listOutputDevices()
		]);
		this.inputDevices = ins;
		this.outputDevices = outs;
	}

	async savePipeline(p: Pipeline): Promise<void> {
		await this.repo.save(p);
		await this.refreshPipelines();
	}

	async deletePipeline(id: string): Promise<void> {
		await this.repo.remove(id);
		await this.refreshPipelines();
	}
}

const STORE_KEY = Symbol('app-store');

export function provideAppStore(store: AppStore): void {
	setContext(STORE_KEY, store);
}

export function useAppStore(): AppStore {
	const store = getContext<AppStore | undefined>(STORE_KEY);
	if (!store) throw new Error('AppStore not provided — wrap in +layout.svelte');
	return store;
}
