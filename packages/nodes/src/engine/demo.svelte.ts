import type { DeviceInfoState, DeviceVolumeState, EnginePort } from './port';
import type { AudioDevice } from './types';
import type { EmitFn } from './demo_signal';

/** A port with no engine behind it: commands do nothing and events come from `emit`. */
export interface DemoPort extends EnginePort {
	emit: EmitFn;
}

const SAMPLE_RATE = 48_000;

const INPUTS: AudioDevice[] = [
	{ id: 'in-1', name: 'Studio Mic', kind: 'input' },
	{ id: 'in-2', name: 'Audio Interface 1-2', kind: 'input' }
];

const OUTPUTS: AudioDevice[] = [
	{ id: 'out-1', name: 'Studio Monitors', kind: 'output' },
	{ id: 'out-2', name: 'Headphones', kind: 'output' }
];

const done = (): Promise<void> => Promise.resolve();

function staticDeviceInfo(): DeviceInfoState {
	return {
		info: { sampleRate: SAMPLE_RATE, channels: 2, sampleFormat: 'f32' },
		isLoading: false,
		sampleRate: SAMPLE_RATE,
		channels: 2,
		sampleFormat: 'f32',
		specText: '48 kHz · 2 ch · f32',
		resamplingTooltip: undefined,
		refresh: done
	};
}

function localVolume(): DeviceVolumeState {
	let scalar = $state(0.8);
	return {
		get scalar() {
			return scalar;
		},
		db: null,
		unsupported: false,
		unsynced: false,
		async set(v) {
			scalar = v;
		}
	};
}

/** For a landing page or a story: nodes render and move without the desktop app. */
export function createDemoPort(options: { running?: boolean; os?: string } = {}): DemoPort {
	const handlers = new Map<string, Set<(payload: unknown) => void>>();

	return {
		isRunning: options.running ?? true,
		sampleRate: SAMPLE_RATE,
		bufferFrames: 256,
		os: options.os ?? 'macos',
		nodeTiming: () => undefined,
		updateEffect: done,
		listen<T>(event: string, handler: (payload: T) => void) {
			const set = handlers.get(event) ?? new Set();
			handlers.set(event, set);
			const h = handler as (payload: unknown) => void;
			set.add(h);
			return () => set.delete(h);
		},
		emit(event, payload) {
			handlers.get(event)?.forEach((h) => h(payload));
		},
		audio: {
			updateEffect: done,
			setInputVolume: done,
			seekAudioFile: done,
			setAudioFileLoop: done,
			setAudioFilePaused: done,
			checkCapturePermission: async () => ({ kind: 'none', state: 'allowed' }),
			openPluginEditor: done,
			closePluginEditor: done,
			getPluginState: async () => null,
			getPluginParams: async () => [],
			pluginStatus: async () => ({ path: null, hasEditor: false }),
			onPluginEditorClosed: async () => () => {},
			netReceiverListen: done,
			netReceiverStats: async () => null,
			netSenderStats: async () => null,
			webrtcHashPassword: async () => '',
			webrtcCreateRoom: async () => 'DEMO42',
			webrtcJoinRoom: done,
			webrtcLeaveRoom: done,
			webrtcDisconnectPeer: done,
			webrtcSetPeerMuted: done,
			webrtcSetIdentity: done,
			webrtcPeerStats: async () => ({}),
			webrtcBufferMs: async () => 0,
			webrtcSessionState: async () => ({ phase: 'idle', roomCode: null, peers: [] }),
			onWebrtcConnected: async () => () => {},
			onWebrtcDisconnected: async () => () => {},
			onWebrtcError: async () => () => {},
			onWebrtcMeta: async () => () => {}
		},
		inputDevices: INPUTS,
		outputDevices: OUTPUTS,
		audioApplications: [],
		refreshInputDevices: done,
		refreshOutputDevices: done,
		refreshAudioApplications: done,
		deviceInfo: staticDeviceInfo,
		deviceVolume: localVolume,
		playCue: done,
		plugins: { plugins: [], scanning: false, scanned: true, error: null, refresh: done, ensure: done, rescan: done },
		readFilePeaks: async () => ({ sampleRate: SAMPLE_RATE, channels: 2, totalFrames: 0, startFrame: 0, mins: [[], []], maxs: [[], []] }),
		missingFilePaths: new Set(),
		chooseFileRequest: null,
		clearChooseFileRequest: () => {},
		retryWithOverwrite: () => {},
		confirmOverwriteChanges: false,
		pickFile: async () => null,
		saveFile: async () => null,
		revealInFolder: done,
		openUrl: done,
		openVirtualDevices: done,
		confirm: async () => true,
		registerHotkey: done,
		unregisterHotkey: done,
		presets: {
			list: async () => [],
			create: async () => {
				throw new Error('Presets are not saved in the demo');
			},
			remove: done,
			promptName: async () => null
		}
	};
}
