import type { EnginePort } from '@splitwave/nodes/engine';
import { tauriListen } from '$lib/utils/tauri_event';
import { appSettings } from '$lib/modules/settings/stores.svelte';
import { methods } from './methods';
import { audioStore } from './stores.svelte';
import { latencyStore } from './latency.svelte';
import { methods as presetMethods } from '$lib/modules/preset';
import { SavePresetModal } from '$lib/modules/preset/ui';
import { modalManager } from '$lib/modules/overlay/modal';

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
	listen: (event, handler) => tauriListen(event, handler),
	get outputDevices() {
		return audioStore.outputDevices;
	},
	refreshOutputDevices: () => audioStore.refreshOutputDevices(),
	playCue: (deviceId, muted, gain, beep) => methods.playCue(deviceId, muted, gain, beep),
	presets: {
		list: (kind) => presetMethods.list(kind),
		create: (kind, name, data) => presetMethods.create(kind, name, data),
		remove: (id) => presetMethods.remove(id),
		promptName: async (taken) => (await modalManager.open<string>('Save preset', SavePresetModal, { size: 'sm', taken })) ?? null
	}
};
