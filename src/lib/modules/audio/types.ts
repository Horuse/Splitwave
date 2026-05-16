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

export interface NativeDeviceInfo {
	sampleRate: number;
	channels: number;
	sampleFormat: string;
}

export type PermissionState = 'allowed' | 'denied' | 'unknown';

export interface VirtualDriverStatus {
	installed: boolean;
}

export interface VirtualDeviceConfig {
	id: string;
	name: string;
}

export type AudioStateEvent =
	| { kind: 'started' }
	| { kind: 'stopped' }
	| { kind: 'error'; message: string };

export interface StartPipelinePayload {
	nodes: PipelineNode[];
	edges: PipelineEdge[];
}
