import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { AudioDevice } from '$lib/domain/device';
import type { PipelineEdge, PipelineNode } from '$lib/domain/audio-node';

export type AudioStateEvent =
	| { kind: 'started' }
	| { kind: 'stopped' }
	| { kind: 'error'; message: string };

export interface StartPipelinePayload {
	nodes: PipelineNode[];
	edges: PipelineEdge[];
}

const AUDIO_STATE_EVENT = 'audio://state';

export class TauriAudioService {
	listInputDevices(): Promise<AudioDevice[]> {
		return invoke<AudioDevice[]>('list_input_devices');
	}

	listOutputDevices(): Promise<AudioDevice[]> {
		return invoke<AudioDevice[]>('list_output_devices');
	}

	startPipeline(graph: StartPipelinePayload): Promise<void> {
		return invoke('start_pipeline', { graph });
	}

	stopPipeline(): Promise<void> {
		return invoke('stop_pipeline');
	}

	onState(cb: (e: AudioStateEvent) => void): Promise<UnlistenFn> {
		return listen<AudioStateEvent>(AUDIO_STATE_EVENT, (evt) => cb(evt.payload));
	}
}
