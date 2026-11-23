import { getContext, setContext } from 'svelte';
import type { NodeTiming } from '../generated/NodeTiming';
import type { Preset, PresetData, PresetKind } from '../presets';
import type { PluginFormat } from '../generated/PluginFormat';
import type { PortLayout } from '../generated/PortLayout';
import type { AudioApplication, AudioDevice, CapturePermission, DeviceKind, NativeDeviceInfo, PluginDescriptor, PluginParam, PluginStatus } from './types';

/** Where user presets live; factory presets ship in the package. */
export interface PresetStore {
	list(kind: PresetKind): Promise<Preset[]>;
	create<K extends PresetKind>(kind: K, name: string, data: PresetData<K>): Promise<Preset<K>>;
	remove(id: string): Promise<void>;
	/** Asks the user for a name; null when they cancel. */
	promptName(taken: string[]): Promise<string | null>;
}

export type DeviceInfoTarget = { kind: 'input' | 'output'; deviceId: () => string | null } | { kind: 'system' | 'app'; pipelineRate?: () => number };

export interface DeviceInfoState {
	readonly info: NativeDeviceInfo | null;
	readonly isLoading: boolean;
	readonly sampleRate: number;
	readonly channels: number;
	readonly sampleFormat: string;
	readonly specText: string;
	readonly resamplingTooltip: string | undefined;
	refresh(): Promise<void>;
}

export interface DeviceVolumeState {
	/** `null` while unknown or unsupported. */
	readonly scalar: number | null;
	/** Device attenuation in dB; `null` when the backend can't report it. */
	readonly db: number | null;
	/** The device exposes no software-settable volume. */
	readonly unsupported: boolean;
	/** The OS reports no volume changes for this device, so the value can go stale. */
	readonly unsynced: boolean;
	set(scalar: number): Promise<void>;
}

/** Installed plugins, shared by every plugin node so the host scans once. */
export interface PluginCatalog {
	readonly plugins: PluginDescriptor[];
	readonly scanning: boolean;
	readonly scanned: boolean;
	readonly error: string | null;
	refresh(): Promise<void>;
	ensure(): Promise<void>;
	rescan(): Promise<void>;
}

export interface NetReceiverStats {
	bytes: number;
	packets: number;
	lost: number;
	channels: number;
	bufferMs: number;
	sampleRate: number;
	format: 'pcm-f32' | 'pcm-i16' | 'opus' | null;
	opusBitrate: number | null;
	opusApp: 'voip' | 'audio' | 'low-delay' | null;
}

export interface WebrtcSessionState {
	phase: 'idle' | 'hosting' | 'joining';
	roomCode: string | null;
	peers: { peerId: string; muted: boolean; name: string; channels: number[] }[];
}

/** Engine commands that a node sends for its own node id. */
export interface AudioCommands {
	updateEffect(nodeId: string, data: Record<string, unknown>): Promise<void>;
	setInputVolume(nodeId: string, scalar: number): Promise<void>;
	seekAudioFile(nodeId: string, frame: number): Promise<void>;
	setAudioFileLoop(nodeId: string, enabled: boolean): Promise<void>;
	setAudioFilePaused(nodeId: string, paused: boolean): Promise<void>;
	checkCapturePermission(): Promise<CapturePermission>;
	openPluginEditor(nodeId: string, title: string): Promise<void>;
	closePluginEditor(nodeId: string): Promise<void>;
	getPluginState(nodeId: string): Promise<string | null>;
	getPluginParams(nodeId: string): Promise<PluginParam[]>;
	pluginStatus(nodeId: string): Promise<PluginStatus>;
	probePluginPorts(format: PluginFormat, path: string, pluginId: string): Promise<PortLayout>;
	onPluginEditorClosed(cb: (nodeId: string) => void): Promise<() => void>;
	netReceiverListen(nodeId: string, port: number): Promise<void>;
	netReceiverStats(nodeId: string): Promise<NetReceiverStats | null>;
	netSenderStats(nodeId: string): Promise<{ bytes: number; packets: number } | null>;
	webrtcHashPassword(password: string): Promise<string>;
	webrtcCreateRoom(nodeId: string, opusBitrate: number, opusApplication: string, passwordHash: string): Promise<string>;
	webrtcJoinRoom(nodeId: string, roomCode: string, opusBitrate: number, opusApplication: string, passwordHash: string): Promise<void>;
	webrtcLeaveRoom(nodeId: string): Promise<void>;
	webrtcDisconnectPeer(nodeId: string, peerId: string): Promise<void>;
	webrtcSetPeerMuted(nodeId: string, peerId: string, muted: boolean): Promise<void>;
	webrtcSetIdentity(nodeId: string, name: string, channels: number, codec: string, opusBitrate: number, opusApplication: string): Promise<void>;
	webrtcPeerStats(nodeId: string): Promise<Record<string, { pingMs: number; packets: number; lost: number }>>;
	webrtcBufferMs(nodeId: string): Promise<number>;
	webrtcSessionState(nodeId: string): Promise<WebrtcSessionState>;
	onWebrtcConnected(cb: (e: { nodeId: string; peerId: string }) => void): Promise<() => void>;
	onWebrtcDisconnected(cb: (e: { nodeId: string; peerId: string }) => void): Promise<() => void>;
	onWebrtcError(cb: (e: { nodeId: string; error: string }) => void): Promise<() => void>;
	onWebrtcMeta(cb: (e: { nodeId: string; peerId: string; name: string; channels: number }) => void): Promise<() => void>;
}

export interface ConfirmOptions {
	message: string;
	confirmLabel: string;
	danger?: boolean;
	canClose?: boolean;
	checkboxLabel?: string;
	warning?: string;
}

/** With a checkbox the answer carries its state; without one it is a plain yes or no. */
export type ConfirmResult = boolean | { ok: boolean; dontAskAgain: boolean };

export interface FileFilter {
	name: string;
	extensions: string[];
}

export interface OpenFileOptions {
	title?: string;
	directory?: boolean;
	filters?: FileFilter[];
}

export interface SaveFileOptions {
	title?: string;
	filters?: FileFilter[];
}

/** A failed start asked a File Recording node to pick another file and retry. */
export interface ChooseFileRequest {
	nodeId: string;
	retryPipelineId: string | null;
}

/**
 * Everything a node needs from the host. The desktop app backs it with the
 * Tauri engine; a landing page or a story backs it with a fake signal.
 * State is read through getters so a component keeps tracking it: never
 * destructure the port.
 */
export interface EnginePort {
	readonly isRunning: boolean;
	/** Rate the pipeline processes at, Hz. */
	readonly sampleRate: number;
	/** Engine buffer while a pipeline runs; null before the first report. */
	readonly bufferFrames: number | null;
	/** Host OS as the platform reports it: 'macos', 'windows', 'linux'. */
	readonly os: string;
	nodeTiming(nodeId: string): NodeTiming | undefined;
	updateEffect(nodeId: string, patch: Record<string, unknown>): Promise<void>;
	/** Subscribes to an engine event. Called during component init it also unsubscribes on destroy. */
	listen<T>(event: string, handler: (payload: T) => void): () => void;
	readonly audio: AudioCommands;

	readonly inputDevices: AudioDevice[];
	readonly outputDevices: AudioDevice[];
	readonly audioApplications: AudioApplication[];
	refreshInputDevices(): Promise<void>;
	refreshOutputDevices(): Promise<void>;
	refreshAudioApplications(): Promise<void>;
	/** Call during component init: the state follows the device while the component lives. */
	deviceInfo(target: DeviceInfoTarget): DeviceInfoState;
	/** Call during component init: the state follows the device while the component lives. */
	deviceVolume(kind: DeviceKind, deviceId: () => string | null): DeviceVolumeState;
	playCue(deviceId: string, muted: boolean, gain: number, beep?: boolean): Promise<void>;
	readonly plugins: PluginCatalog;

	readFilePeaks(
		path: string,
		startFrame: number,
		framesPerBin: number,
		binCount: number
	): Promise<{ sampleRate: number; channels: number; totalFrames: number; startFrame: number; mins: number[][]; maxs: number[][] }>;
	/** Audio files the last start could not find. */
	readonly missingFilePaths: ReadonlySet<string>;
	readonly chooseFileRequest: ChooseFileRequest | null;
	clearChooseFileRequest(): void;
	/** Restarts the pipeline with this File Recording node switched to Overwrite. */
	retryWithOverwrite(pipelineId: string, nodeId: string): void;
	/** User setting: ask before a change erases an Overwrite recording. */
	readonly confirmOverwriteChanges: boolean;

	pickFile(options: OpenFileOptions): Promise<string | null>;
	saveFile(options: SaveFileOptions): Promise<string | null>;
	revealInFolder(path: string): Promise<void>;
	openUrl(url: string): Promise<void>;
	openVirtualDevices(): Promise<void>;
	confirm(title: string, options: ConfirmOptions): Promise<ConfirmResult | undefined>;
	registerHotkey(key: string, onEvent: (state: 'Pressed' | 'Released') => void): Promise<void>;
	unregisterHotkey(key: string): Promise<void>;

	readonly presets: PresetStore;
}

const KEY = Symbol('engine-port');

export function setEnginePort(port: EnginePort): void {
	setContext(KEY, port);
}

/** Throws when no host provided a port: a node rendered without an engine is a wiring bug, not a state to paper over. */
export function getEnginePort(): EnginePort {
	const port = getContext<EnginePort | undefined>(KEY);
	if (!port) throw new Error('EnginePort missing: wrap nodes in a host that calls setEnginePort()');
	return port;
}
