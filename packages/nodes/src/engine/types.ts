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
