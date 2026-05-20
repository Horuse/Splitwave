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
	isPipelineRunning: (): Promise<boolean> => invoke<boolean>('is_pipeline_running'),
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
	/** Pause or resume an AudioFile input. No-op when not running. */
	setAudioFilePaused: (nodeId: string, paused: boolean): Promise<void> =>
		invoke('set_audio_file_paused', { nodeId, paused }),
	/** Live volume update for an input node (App Audio, System Audio, Audio File). No-op when not running. */
	setInputVolume: (nodeId: string, scalar: number): Promise<void> =>
		invoke('set_input_volume', { nodeId, scalar }),
	/** `null` when the device has no software-settable volume in that scope. */
	getDeviceVolume: (kind: 'input' | 'output', name: string): Promise<number | null> =>
		invoke<number | null>('get_device_volume', { kind, name }),
	/** Throws when not settable. */
	setDeviceVolume: (kind: 'input' | 'output', name: string, scalar: number): Promise<void> =>
		invoke('set_device_volume', { kind, name, scalar }),
	onState: (cb: (e: AudioStateEvent) => void): Promise<UnlistenFn> =>
		listen<AudioStateEvent>(AUDIO_STATE_EVENT, (evt) => cb(evt.payload)),
	onSpeakerError: (cb: () => void): Promise<UnlistenFn> =>
		listen('audio://speaker_error', () => cb()),
	virtualDriverStatus: (): Promise<VirtualDriverStatus> =>
		invoke<VirtualDriverStatus>('virtual_driver_status'),
	installVirtualDriver: (): Promise<void> => invoke('install_virtual_driver'),
	uninstallVirtualDriver: (): Promise<void> => invoke('uninstall_virtual_driver'),
	applyVirtualDevices: (devices: VirtualDeviceConfig[]): Promise<void> =>
		invoke('apply_virtual_devices', { devices }),
	webrtcCreateOffer: (
		nodeId: string,
		opusBitrate: number,
		opusApplication: string
	): Promise<{ peerId: string; offerCode: string }> =>
		invoke('webrtc_create_offer', { nodeId, opusBitrate, opusApplication }),
	webrtcAcceptOffer: (
		nodeId: string,
		offerCode: string,
		opusBitrate: number,
		opusApplication: string
	): Promise<{ peerId: string; answerCode: string }> =>
		invoke('webrtc_accept_offer', { nodeId, offerCode, opusBitrate, opusApplication }),
	webrtcCompleteHandshake: (nodeId: string, answerCode: string): Promise<void> =>
		invoke('webrtc_complete_handshake', { nodeId, answerCode }),
	webrtcDisconnectPeer: (nodeId: string, peerId: string): Promise<void> =>
		invoke('webrtc_disconnect_peer', { nodeId, peerId }),
	webrtcSetPeerMuted: (nodeId: string, peerId: string, muted: boolean): Promise<void> =>
		invoke('webrtc_set_peer_muted', { nodeId, peerId, muted }),
	onWebrtcConnected: (cb: (e: { nodeId: string; peerId: string }) => void): Promise<UnlistenFn> =>
		listen<{ nodeId: string; peerId: string }>('audio://webrtc_connected', (evt) =>
			cb(evt.payload)
		),
	onWebrtcDisconnected: (
		cb: (e: { nodeId: string; peerId: string }) => void
	): Promise<UnlistenFn> =>
		listen<{ nodeId: string; peerId: string }>('audio://webrtc_disconnected', (evt) =>
			cb(evt.payload)
		),
	onWebrtcError: (cb: (e: { nodeId: string; error: string }) => void): Promise<UnlistenFn> =>
		listen<{ nodeId: string; error: string }>('audio://webrtc_error', (evt) =>
			cb(evt.payload)
		),
	/** Host: creates a room and returns the 6-char room code. Connection completes via onWebrtcConnected. */
	webrtcCreateRoom: (
		nodeId: string,
		opusBitrate: number,
		opusApplication: string
	): Promise<string> => invoke<string>('webrtc_create_room', { nodeId, opusBitrate, opusApplication }),
	/** Returns RTT ping in ms per display peer ID. 0 = no ping yet. */
	webrtcPeerPings: (nodeId: string): Promise<Record<string, number>> =>
		invoke<Record<string, number>>('webrtc_peer_pings', { nodeId }),
	/** Current room phase/code and connected peers, for restoring node UI on remount. */
	webrtcSessionState: (
		nodeId: string
	): Promise<{
		phase: 'idle' | 'hosting' | 'joining';
		roomCode: string | null;
		peers: { peerId: string; muted: boolean }[];
	}> => invoke('webrtc_session_state', { nodeId }),
	/** Guest: joins a room by code. Connection completes via onWebrtcConnected. */
	webrtcJoinRoom: (
		nodeId: string,
		roomCode: string,
		opusBitrate: number,
		opusApplication: string
	): Promise<void> => invoke('webrtc_join_room', { nodeId, roomCode, opusBitrate, opusApplication })
};
