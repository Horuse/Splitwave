import type { EnginePort } from '@splitwave/nodes/engine';
import { open, save } from '@tauri-apps/plugin-dialog';
import { openUrl, revealItemInDir } from '@tauri-apps/plugin-opener';
import { platform } from '@tauri-apps/plugin-os';
import { register, unregister } from '@tauri-apps/plugin-global-shortcut';
import { goto } from '$app/navigation';
import { tauriListen } from '$lib/utils/tauri_event';
import { appSettings } from '$lib/modules/settings/stores.svelte';
import { pipelineStore } from '$lib/modules/pipeline/stores.svelte';
import { methods } from './methods';
import { audioStore } from './stores.svelte';
import { latencyStore } from './latency.svelte';
import { useDeviceInfo } from './device_info.svelte';
import { deviceVolume } from './device_volume.svelte';
import { pluginCatalog } from './plugin_catalog.svelte';
import { methods as presetMethods } from '$lib/modules/preset';
import { SavePresetModal } from '$lib/modules/preset/ui';
import { modalManager } from '$lib/modules/overlay/modal';
import { ConfirmModal } from '$lib/modules/overlay/ui';

export const appEnginePort: EnginePort = {
	get isRunning() {
		return audioStore.editorIsLive;
	},
	get sampleRate() {
		return appSettings.pipelineSampleRate;
	},
	get bufferFrames() {
		return audioStore.editorIsLive ? (latencyStore.report?.bufferFrames ?? null) : null;
	},
	os: platform(),
	nodeTiming: (nodeId) => (audioStore.editorIsLive ? latencyStore.node(nodeId) : undefined),
	updateEffect: (nodeId, patch) => methods.updateEffect(nodeId, patch),
	listen: (event, handler) => tauriListen(event, handler),
	audio: methods,

	get inputDevices() {
		return audioStore.inputDevices;
	},
	get outputDevices() {
		return audioStore.outputDevices;
	},
	get audioApplications() {
		return audioStore.audioApplications;
	},
	refreshInputDevices: () => audioStore.refreshInputDevices(),
	refreshOutputDevices: () => audioStore.refreshOutputDevices(),
	refreshAudioApplications: () => audioStore.refreshAudioApplications(),
	deviceInfo: (target) => useDeviceInfo(target),
	deviceVolume: (kind, deviceId) => deviceVolume(kind, deviceId),
	playCue: (deviceId, muted, gain, beep) => methods.playCue(deviceId, muted, gain, beep),
	plugins: pluginCatalog,

	readFilePeaks: (path, startFrame, framesPerBin, binCount) => methods.readFilePeaks(path, startFrame, framesPerBin, binCount),
	get missingFilePaths() {
		return audioStore.missingFilePaths;
	},
	get chooseFileRequest() {
		const nodeId = audioStore.chooseFileNodeId;
		return nodeId === null ? null : { nodeId, retryPipelineId: audioStore.pendingRetryPipelineId };
	},
	clearChooseFileRequest() {
		audioStore.chooseFileNodeId = null;
		audioStore.pendingRetryPipelineId = null;
	},
	retryWithOverwrite(pipelineId, nodeId) {
		const snapshot = pipelineStore.editorActions?.getSnapshot();
		if (!snapshot) return;
		const nodes = snapshot.nodes.map((n) => (n.id === nodeId ? { ...n, data: { ...n.data, mode: 'overwrite' } } : n));
		audioStore.activatePipeline(pipelineId, { nodes, edges: snapshot.edges }).catch((e) => audioStore.reportError(e));
	},
	get confirmOverwriteChanges() {
		return appSettings.confirmOverwriteChanges;
	},

	pickFile: async (options) => {
		const path = await open({ ...options, multiple: false });
		return typeof path === 'string' ? path : null;
	},
	saveFile: async (options) => (await save(options)) ?? null,
	revealInFolder: (path) => revealItemInDir(path),
	openUrl: (url) => openUrl(url),
	openVirtualDevices: () => goto('/virtual-devices'),
	confirm: (title, options) => modalManager.open(title, ConfirmModal, { ...options }),
	registerHotkey: (key, onEvent) => register(key, (e) => onEvent(e.state)),
	unregisterHotkey: (key) => unregister(key),

	presets: {
		list: (kind) => presetMethods.list(kind),
		create: (kind, name, data) => presetMethods.create(kind, name, data),
		remove: (id) => presetMethods.remove(id),
		promptName: async (taken) => (await modalManager.open<string>('Save preset', SavePresetModal, { size: 'sm', taken })) ?? null
	}
};
