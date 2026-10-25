import { mockIPC } from '@tauri-apps/api/mocks';
import type { AudioApplication, AudioDevice, PluginDescriptor, PluginParam } from '$lib/modules/audio/types';
import { audioStore } from '$lib/modules/audio/stores.svelte';
import { appSettings } from '$lib/modules/settings/stores.svelte';
import type { LatencyReport } from '$lib/modules/pipeline/generated/LatencyReport';

export type Platform = 'macos' | 'windows' | 'linux';
export type LinkQuality = 'offline' | 'good' | 'lossy';

/** Everything the node UI reads from the engine, driven by story controls. */
export interface MockEnv {
	platform: Platform;
	running: boolean;
	pipelineSampleRate: number;
	inputDevices: number;
	outputDevices: number;
	deviceChannels: number;
	deviceSampleRate: number;
	deviceVolume: 'supported' | 'unsupported' | 'unsynced';
	apps: number;
	capturePermission: 'none' | 'allowed' | 'denied' | 'unknown';
	plugins: number;
	pluginParams: number;
	pluginEditor: boolean;
	link: LinkQuality;
	linkChannels: number;
	webrtcPhase: 'idle' | 'hosting' | 'joining';
	webrtcPeers: number;
	missingFile: boolean;
	bufferFrames: number;
	/** Block the story node reports running at; 0 when it keeps up. */
	workingBlock: number;
}

/** Id of the node a story renders; the engine mocks answer for it. */
export const STORY_NODE_ID = 'story-node';

export const DEFAULT_ENV: MockEnv = {
	platform: 'macos',
	running: false,
	pipelineSampleRate: 48_000,
	inputDevices: 3,
	outputDevices: 3,
	deviceChannels: 2,
	deviceSampleRate: 48_000,
	deviceVolume: 'supported',
	apps: 4,
	capturePermission: 'none',
	plugins: 4,
	pluginParams: 6,
	pluginEditor: true,
	link: 'good',
	linkChannels: 2,
	webrtcPhase: 'idle',
	webrtcPeers: 0,
	missingFile: false,
	bufferFrames: 256,
	workingBlock: 0
};

const INPUT_NAMES = ['MacBook Pro Microphone', 'Scarlett 2i2 USB', 'Splitwave', 'AirPods Pro', 'Shure MV7', 'RODECaster Pro II'];
const OUTPUT_NAMES = ['MacBook Pro Speakers', 'Scarlett 2i2 USB', 'Splitwave', 'AirPods Pro', 'LG UltraFine Display', 'BlackHole 16ch'];
const APP_NAMES = ['Spotify', 'Google Chrome', 'Discord', 'Zoom', 'Music', 'OBS Studio', 'Firefox', 'Slack'];
const PLUGINS = [
	{ name: 'Pro-Q 3', vendor: 'FabFilter', format: 'clap' as const },
	{ name: 'Valhalla Supermassive', vendor: 'Valhalla DSP', format: 'clap' as const },
	{ name: 'AUDelay', vendor: 'Apple', format: 'au' as const },
	{ name: 'Surge XT', vendor: 'Surge Synth Team', format: 'clap' as const },
	{ name: 'OTT', vendor: 'Xfer Records', format: 'au' as const },
	{ name: 'Vital', vendor: 'Matt Tytel', format: 'clap' as const }
];
const PARAM_NAMES = ['Gain', 'Mix', 'Frequency', 'Resonance', 'Attack', 'Release', 'Drive', 'Width', 'Output', 'Mode'];
const PEER_NAMES = ['Alex', 'Sam', 'Jordan', 'Riley', 'Casey'];

function take<T>(names: T[], count: number): T[] {
	return Array.from({ length: count }, (_, i) => names[i % names.length]);
}

export function mockInputDevices(env: MockEnv): AudioDevice[] {
	return take(INPUT_NAMES, env.inputDevices).map((name, i) => ({ id: `in-${i + 1}`, name, kind: 'input' }));
}

export function mockOutputDevices(env: MockEnv): AudioDevice[] {
	return take(OUTPUT_NAMES, env.outputDevices).map((name, i) => ({ id: `out-${i + 1}`, name, kind: 'output' }));
}

function mockApps(env: MockEnv): AudioApplication[] {
	return take(APP_NAMES, env.apps).map((name, i) => ({ bundleId: `com.example.app${i + 1}`, name }));
}

export function mockPlugins(env: MockEnv): PluginDescriptor[] {
	return take(PLUGINS, env.plugins).map((p, i) => ({
		uid: `plugin-${i + 1}`,
		format: p.format,
		path: `/Library/Audio/Plug-Ins/${p.format.toUpperCase()}/${p.name}.${p.format === 'clap' ? 'clap' : 'component'}`,
		pluginId: `com.example.plugin${i + 1}`,
		name: p.name,
		vendor: p.vendor,
		version: '1.0.0'
	}));
}

function mockParams(count: number): PluginParam[] {
	return take(PARAM_NAMES, count).map((name, i) => {
		const stepped = name === 'Mode';
		return {
			id: i,
			name,
			min: 0,
			max: stepped ? 3 : 1,
			default: stepped ? 0 : 0.5,
			value: stepped ? 1 : ((i * 37) % 100) / 100,
			step: stepped ? 1 : 0,
			readOnly: false
		};
	});
}

/** Node data the plugin status probe answers with; set by the canvas. */
let nodeData: (id: string) => Record<string, unknown> | undefined = () => undefined;

export function setNodeDataLookup(lookup: typeof nodeData): void {
	nodeData = lookup;
}

interface Counters {
	startedAt: number;
}

function linkStats(env: MockEnv, counters: Counters) {
	if (env.link === 'offline' || !env.running) return null;
	const seconds = (performance.now() - counters.startedAt) / 1000;
	const packets = Math.floor(seconds * 50);
	const lost = env.link === 'lossy' ? Math.floor(packets * 0.04) : 0;
	return { packets, lost, bytes: Math.floor(seconds * 12_000 * env.linkChannels) };
}

export function installMockBackend(env: MockEnv): void {
	(window as unknown as Record<string, unknown>).__TAURI_OS_PLUGIN_INTERNALS__ = {
		platform: env.platform,
		family: env.platform === 'windows' ? 'windows' : 'unix',
		os_type: env.platform,
		version: '1.0.0',
		arch: 'aarch64',
		eol: env.platform === 'windows' ? '\r\n' : '\n',
		exe_extension: env.platform === 'windows' ? 'exe' : ''
	};

	const inputs = mockInputDevices(env);
	const outputs = mockOutputDevices(env);
	const apps = mockApps(env);
	const plugins = mockPlugins(env);
	const params = new Map<string, PluginParam[]>();
	const volumes = new Map<string, number>();
	const counters: Counters = { startedAt: performance.now() };
	const peers = take(PEER_NAMES, env.webrtcPeers).map((name, i) => ({
		peerId: `peer-${i + 1}`,
		muted: i === 1,
		name,
		channels: [i % 2 === 0 ? 2 : 1]
	}));

	mockIPC(
		(cmd, payload) => {
			const args = (payload ?? {}) as Record<string, unknown>;
			switch (cmd) {
				case 'list_input_devices':
					return inputs;
				case 'list_output_devices':
					return outputs;
				case 'list_audio_applications':
					return apps;
				case 'get_app_icons':
					return {};
				case 'device_info':
				case 'capture_device_info':
					return { sampleRate: env.deviceSampleRate, channels: env.deviceChannels, sampleFormat: 'f32' };
				case 'get_device_volume': {
					if (env.deviceVolume === 'unsupported') return null;
					const scalar = volumes.get(String(args.name)) ?? 0.75;
					return { scalar, db: 20 * Math.log10(Math.max(scalar, 1e-4)) };
				}
				case 'watch_device_volume':
					if (env.deviceVolume === 'unsynced') throw new Error('volume notifications unavailable');
					return null;
				case 'set_device_volume':
					volumes.set(String(args.name), Number(args.scalar));
					return null;
				case 'check_capture_permission':
					return env.capturePermission === 'none' ? { kind: 'none', state: 'unknown' } : { kind: 'screenrecording', state: env.capturePermission };
				case 'latency_report':
					return mockLatencyReport(env);
				case 'is_pipeline_running':
					return env.running;
				case 'path_exists':
					return !env.missingFile;
				case 'scan_plugins':
					return plugins;
				case 'plugin_status': {
					const path = nodeData(String(args.nodeId))?.path;
					return { path: env.running && path ? path : null, hasEditor: env.pluginEditor };
				}
				case 'get_plugin_params': {
					const id = String(args.nodeId);
					if (!params.has(id)) params.set(id, mockParams(env.pluginParams));
					return params.get(id);
				}
				case 'get_plugin_state':
					return 'bW9jay1zdGF0ZQ==';
				case 'open_plugin_editor':
					if (!env.pluginEditor) throw new Error('Plugin editor failed: this plugin has no GUI');
					return null;
				case 'update_effect': {
					const values = (args.data as Record<string, unknown> | undefined)?.pluginParams as Record<string, number> | undefined;
					const list = params.get(String(args.nodeId));
					if (values && list) for (const [pid, v] of Object.entries(values)) list[Number(pid)].value = v;
					return null;
				}
				case 'net_receiver_stats': {
					const s = linkStats(env, counters);
					if (!s) return null;
					return {
						...s,
						channels: env.linkChannels,
						bufferMs: env.link === 'lossy' ? 60 : 20,
						sampleRate: 48_000,
						format: 'opus',
						opusBitrate: 96_000,
						opusApp: 'audio'
					};
				}
				case 'net_sender_stats': {
					const s = linkStats(env, counters);
					return s && { bytes: s.bytes, packets: s.packets };
				}
				case 'webrtc_session_state':
					return { phase: env.webrtcPhase, roomCode: env.webrtcPhase === 'idle' ? null : 'K7Q2XF', peers: env.webrtcPhase === 'idle' ? [] : peers };
				case 'webrtc_create_room':
					return 'K7Q2XF';
				case 'webrtc_peer_pings':
					return Object.fromEntries(peers.map((p, i) => [p.peerId, 18 + i * 25]));
				case 'webrtc_peer_stats': {
					const s = linkStats(env, counters) ?? { packets: 0, lost: 0 };
					return Object.fromEntries(peers.map((p, i) => [p.peerId, { pingMs: 18 + i * 25, packets: s.packets, lost: s.lost * i }]));
				}
				case 'webrtc_buffer_ms':
					return env.link === 'lossy' ? 80 : 30;
				case 'plugin:dialog|open':
					return '/Users/demo/Music/demo-track.wav';
				case 'plugin:dialog|save':
					return '/Users/demo/Recordings/take-01.wav';
				case 'plugin:store|entries':
				case 'plugin:store|keys':
					return [];
				default:
					return null;
			}
		},
		{ shouldMockEvents: true }
	);

	audioStore.inputDevices = inputs;
	audioStore.outputDevices = outputs;
	audioStore.audioApplications = apps;
	audioStore.isRunning = env.running;
	audioStore.startedAt = env.running ? Date.now() : null;
	audioStore.missingFilePaths = new Set(env.missingFile ? ['/Users/demo/Music/demo-track.wav'] : []);
	appSettings.pipelineSampleRate = env.pipelineSampleRate;
	appSettings.bufferFrames = env.bufferFrames;
}

function mockLatencyReport(env: MockEnv): LatencyReport {
	const ms = (frames: number) => (frames * 1000) / env.pipelineSampleRate;
	const node = env.workingBlock > 0 ? env.workingBlock : 0;
	const path = {
		inputDeviceMs: ms(env.bufferFrames + 400),
		inputQueueMs: ms(env.bufferFrames * 2),
		processingMs: ms(node),
		outputAdapterMs: 0,
		outputDeviceMs: ms(env.bufferFrames + 400),
		hardwareIncluded: env.platform === 'macos'
	};
	return {
		path: { ...path, totalMs: Object.values(path).reduce<number>((a, v) => a + (typeof v === 'number' ? v : 0), 0) },
		bufferFrames: env.bufferFrames,
		sampleRate: env.pipelineSampleRate,
		deviceBufferFrames: env.bufferFrames,
		dspLoad: 0.18,
		overloads: 0,
		nodes: node ? [{ nodeId: STORY_NODE_ID, latencyFrames: node, workingBlock: node, sampleRate: env.pipelineSampleRate }] : []
	};
}
