import type { PipelineEdge, PipelineNode } from '$lib/modules/pipeline/types';

export type DeviceKind = 'input' | 'output';

export interface AudioDevice {
	id: string;
	name: string;
	kind: DeviceKind;
}

export interface AudioApplication {
	bundleId: string;
	name: string;
	/** Base64-encoded PNG icon (data URL body, no scheme prefix). */
	icon?: string | null;
}

export interface PluginDescriptor {
	uid: string;
	format: 'clap' | 'au';
	path: string;
	pluginId: string;
	name: string;
	vendor: string;
	version: string;
}

export interface PluginStatus {
	/** Reference of the plugin actually running, null while none is loaded. */
	path: string | null;
	hasEditor: boolean;
}

export interface PluginParam {
	id: number;
	name: string;
	min: number;
	max: number;
	default: number;
	value: number;
	stepped: boolean;
	readOnly: boolean;
}

export interface NativeDeviceInfo {
	sampleRate: number;
	channels: number;
	sampleFormat: string;
}

/** `db` is the device's own attenuation; `null` when the backend can't report it. */
export interface DeviceVolume {
	scalar: number;
	db: number | null;
}

export interface VolumeChange extends DeviceVolume {
	kind: DeviceKind;
	name: string;
}

export type PermissionState = 'allowed' | 'denied' | 'unknown';

/** `systemaudio` has no preflight API — it resolves on the first capture. */
export type PermissionKind = 'systemaudio' | 'screenrecording' | 'none';

export interface CapturePermission {
	kind: PermissionKind;
	state: PermissionState;
}

export interface VirtualDriverStatus {
	installed: boolean;
	installedVersion: number | null;
	currentVersion: number;
	needsUpdate: boolean;
}

export interface VirtualDeviceConfig {
	id: string;
	name: string;
	channels: number;
}

export type WindowsVirtualCableState =
	| 'notInstalled'
	| 'installedExternal'
	| 'installedManaged'
	| 'partial'
	| 'rebootRequired'
	| 'removalPendingReboot'
	| 'unknownOwnership';

export type WindowsVirtualCableOwnership = 'external' | 'managed' | 'unknown';

export interface WindowsVirtualCableStatus {
	state: WindowsVirtualCableState;
	usable: boolean;
	provider: string;
	installedVersion: string | null;
	renderEndpointName: string | null;
	captureEndpointName: string | null;
	ownership: WindowsVirtualCableOwnership;
	managedBySplitwave: boolean;
	rebootRequired: boolean;
	detail: string | null;
}

export interface WindowsVirtualCableError {
	code: string;
	message: string;
}

export type AudioStateEvent = { kind: 'started' } | { kind: 'stopped' } | { kind: 'error'; message: string };

export interface StartPipelinePayload {
	nodes: PipelineNode[];
	edges: PipelineEdge[];
}
