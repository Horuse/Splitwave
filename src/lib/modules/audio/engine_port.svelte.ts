import type { EnginePort } from '@splitwave/nodes/engine';
import { tauriListen } from '$lib/utils/tauri_event';
import { appSettings } from '$lib/modules/settings/stores.svelte';
import { methods } from './methods';
import { audioStore } from './stores.svelte';
import { latencyStore } from './latency.svelte';

export const appEnginePort: EnginePort = {
	get isRunning() {
		return audioStore.isRunning;
	},
	get sampleRate() {
		return appSettings.pipelineSampleRate;
	},
	get bufferFrames() {
		return latencyStore.report?.bufferFrames ?? null;
	},
	nodeTiming: (nodeId) => latencyStore.node(nodeId),
	updateEffect: (nodeId, patch) => methods.updateEffect(nodeId, patch),
	listen: (event, handler) => tauriListen(event, handler)
};
