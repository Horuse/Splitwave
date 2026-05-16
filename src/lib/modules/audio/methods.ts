import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type {
	AudioApplication,
	AudioDevice,
	AudioStateEvent,
	NativeDeviceInfo,
	PermissionState,
	StartPipelinePayload,
	VirtualDeviceConfig,
	VirtualDriverStatus
} from './types';

const AUDIO_STATE_EVENT = 'audio://state';

export const methods = {
	listInputDevices: (): Promise<AudioDevice[]> => invoke<AudioDevice[]>('list_input_devices'),
	listOutputDevices: (): Promise<AudioDevice[]> => invoke<AudioDevice[]>('list_output_devices'),
	listAudioApplications: (): Promise<AudioApplication[]> =>
		invoke<AudioApplication[]>('list_audio_applications'),
	getAppIcons: (bundleIds: string[]): Promise<Record<string, string>> =>
		invoke<Record<string, string>>('get_app_icons', { bundleIds }),
	deviceInfo: (kind: 'input' | 'output', name: string): Promise<NativeDeviceInfo> =>
		invoke<NativeDeviceInfo>('device_info', { kind, name }),
	checkScreenRecordingPermission: (): Promise<PermissionState> =>
		invoke<PermissionState>('check_screen_recording_permission'),
	startPipeline: (graph: StartPipelinePayload): Promise<void> =>
		invoke('start_pipeline', { graph }),
	stopPipeline: (): Promise<void> => invoke('stop_pipeline'),
	/** Hot-reconfigure a running pipeline. Errors with `NotRunning` if no
	 *  pipeline is active — callers should fall back to `startPipeline`. */
	reconcilePipeline: (graph: StartPipelinePayload): Promise<void> =>
		invoke('reconcile_pipeline', { graph }),
	/** No-op when the pipeline isn't running; callers can fire-and-forget. */
	updateEffect: (nodeId: string, data: Record<string, unknown>): Promise<void> =>
		invoke('update_effect', { nodeId, data }),
	/** Seek an AudioFile input. No-op when not running. */
	seekAudioFile: (nodeId: string, frame: number): Promise<void> =>
		invoke('seek_audio_file', { nodeId, frame }),
	/** Live loop toggle for an AudioFile input. No-op when not running. */
	setAudioFileLoop: (nodeId: string, enabled: boolean): Promise<void> =>
		invoke('set_audio_file_loop', { nodeId, enabled }),
	/** `null` when the device has no software-settable volume in that scope. */
	getDeviceVolume: (kind: 'input' | 'output', name: string): Promise<number | null> =>
		invoke<number | null>('get_device_volume', { kind, name }),
	/** Throws when not settable. */
	setDeviceVolume: (kind: 'input' | 'output', name: string, scalar: number): Promise<void> =>
		invoke('set_device_volume', { kind, name, scalar }),
	onState: (cb: (e: AudioStateEvent) => void): Promise<UnlistenFn> =>
		listen<AudioStateEvent>(AUDIO_STATE_EVENT, (evt) => cb(evt.payload)),
	virtualDriverStatus: (): Promise<VirtualDriverStatus> =>
		invoke<VirtualDriverStatus>('virtual_driver_status'),
	installVirtualDriver: (): Promise<void> => invoke('install_virtual_driver'),
	uninstallVirtualDriver: (): Promise<void> => invoke('uninstall_virtual_driver'),
	applyVirtualDevices: (devices: VirtualDeviceConfig[]): Promise<void> =>
		invoke('apply_virtual_devices', { devices })
};
