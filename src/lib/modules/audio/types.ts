import type { PipelineEdge, PipelineNode } from '@splitwave/nodes';

import type { DeviceKind } from '@splitwave/nodes/engine';
export type {
	AudioApplication,
	AudioDevice,
	DeviceKind,
	PluginDescriptor,
	PluginStatus,
	PluginParam,
	NativeDeviceInfo,
	DeviceVolume,
	PermissionState,
	PermissionKind,
	CapturePermission
} from '@splitwave/nodes/engine';
import type { DeviceVolume } from '@splitwave/nodes/engine';

export interface VolumeChange extends DeviceVolume {
	kind: DeviceKind;
	name: string;
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
	sampleRate?: number;
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
	installerExitCode?: number | null;
}

export type AudioStateEvent = { kind: 'started' } | { kind: 'stopped' } | { kind: 'error'; message: string };

export interface StartPipelinePayload {
	nodes: PipelineNode[];
	edges: PipelineEdge[];
	sampleRate?: number;
}
