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
	step: number;
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

export type PermissionState = 'allowed' | 'denied' | 'unknown';

/** `systemaudio` has no preflight API — it resolves on the first capture. */
export type PermissionKind = 'systemaudio' | 'screenrecording' | 'none';

export interface CapturePermission {
	kind: PermissionKind;
	state: PermissionState;
}
