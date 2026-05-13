import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type {
	AudioApplication,
	AudioDevice,
	AudioStateEvent,
	NativeDeviceInfo,
	PermissionState,
	StartPipelinePayload
} from './types';

const AUDIO_STATE_EVENT = 'audio://state';

export const methods = {
	listInputDevices: (): Promise<AudioDevice[]> => invoke<AudioDevice[]>('list_input_devices'),
	listOutputDevices: (): Promise<AudioDevice[]> => invoke<AudioDevice[]>('list_output_devices'),
	listAudioApplications: (): Promise<AudioApplication[]> =>
		invoke<AudioApplication[]>('list_audio_applications'),
	deviceInfo: (kind: 'input' | 'output', name: string): Promise<NativeDeviceInfo> =>
		invoke<NativeDeviceInfo>('device_info', { kind, name }),
	checkScreenRecordingPermission: (): Promise<PermissionState> =>
		invoke<PermissionState>('check_screen_recording_permission'),
	startPipeline: (graph: StartPipelinePayload): Promise<void> =>
		invoke('start_pipeline', { graph }),
	stopPipeline: (): Promise<void> => invoke('stop_pipeline'),
	/** No-op when the pipeline isn't running; callers can fire-and-forget. */
	updateEffect: (nodeId: string, data: Record<string, unknown>): Promise<void> =>
		invoke('update_effect', { nodeId, data }),
	/** `null` when the device has no software-settable volume in that scope. */
	getDeviceVolume: (kind: 'input' | 'output', name: string): Promise<number | null> =>
		invoke<number | null>('get_device_volume', { kind, name }),
	/** Throws when not settable. */
	setDeviceVolume: (kind: 'input' | 'output', name: string, scalar: number): Promise<void> =>
		invoke('set_device_volume', { kind, name, scalar }),
	onState: (cb: (e: AudioStateEvent) => void): Promise<UnlistenFn> =>
		listen<AudioStateEvent>(AUDIO_STATE_EVENT, (evt) => cb(evt.payload))
};
