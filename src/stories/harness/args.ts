import type { InputType } from 'storybook/internal/types';
import { DEFAULT_ENV, type MockEnv } from './backend';
import { DEFAULT_SIGNAL, SIGNAL_SHAPES, type SignalOptions } from './signal';

type ArgTypes = Record<string, InputType>;

/** Channels cabled into the node from a stub source; grows channel sockets. */
export const WIRED_INPUTS = 'wiredInputs';

const range = (min: number, max: number, step = 1) => ({ type: 'range' as const, min, max, step });

const SIGNAL_ARG_TYPES: Record<keyof SignalOptions, InputType> = {
	signal: { control: 'select', options: SIGNAL_SHAPES, description: 'Fake stream shape; `off` sends nothing' },
	signalLevel: { control: range(-60, 6), description: 'Peak level, dBFS' },
	signalFrequency: { control: range(20, 20_000, 10), description: 'Sine frequency, Hz' },
	signalChannels: { control: range(1, 8), description: 'Channels in the fake stream' },
	gainReduction: { control: range(0, 24, 0.5), description: 'Reported gain reduction, dB' },
	signalMotion: { control: 'boolean', description: 'Swing level and gain reduction' },
	signalClipping: { control: 'boolean', description: 'Overdrive into full-scale clipping' }
};

const ENV_ARG_TYPES: Record<keyof MockEnv, InputType> = {
	platform: { control: 'inline-radio', options: ['macos', 'windows', 'linux'] },
	running: { control: 'boolean', description: 'Pipeline active' },
	pipelineSampleRate: { control: 'select', options: [44_100, 48_000, 88_200, 96_000] },
	inputDevices: { control: range(0, 6) },
	outputDevices: { control: range(0, 6) },
	deviceChannels: { control: range(1, 16) },
	deviceSampleRate: { control: 'select', options: [44_100, 48_000, 88_200, 96_000] },
	deviceVolume: { control: 'inline-radio', options: ['supported', 'unsupported', 'unsynced'] },
	apps: { control: range(0, 8) },
	capturePermission: { control: 'inline-radio', options: ['none', 'allowed', 'denied', 'unknown'] },
	plugins: { control: range(0, 6) },
	pluginParams: { control: range(0, 10) },
	pluginEditor: { control: 'boolean', description: 'Plugin exposes a GUI' },
	link: { control: 'inline-radio', options: ['offline', 'good', 'lossy'] },
	linkChannels: { control: range(1, 8) },
	webrtcPhase: { control: 'inline-radio', options: ['idle', 'hosting', 'joining'] },
	webrtcPeers: { control: range(0, 5) },
	missingFile: { control: 'boolean', description: 'Chosen file no longer exists' },
	bufferFrames: { control: 'select', options: [32, 64, 128, 256, 512, 1024, 2048], description: 'Engine buffer, frames' },
	workingBlock: { control: 'select', options: [0, 256, 480, 512], description: 'Block the node reports running at; 0 keeps up' }
};

function withCategory(types: Record<string, InputType>, category: string): ArgTypes {
	return Object.fromEntries(Object.entries(types).map(([k, v]) => [k, { ...v, table: { category } }]));
}

/** Controls for the fake stream; all of them unless a subset is named. */
export function signalArgs(keys: (keyof SignalOptions)[] = Object.keys(SIGNAL_ARG_TYPES) as (keyof SignalOptions)[]) {
	const types = Object.fromEntries(keys.map((k) => [k, SIGNAL_ARG_TYPES[k]]));
	const args = Object.fromEntries(keys.map((k) => [k, DEFAULT_SIGNAL[k]]));
	return { argTypes: withCategory(types, 'Signal'), args };
}

/** Controls for the mocked engine; only the named ones are shown. */
export function envArgs(keys: (keyof MockEnv)[]) {
	const types = Object.fromEntries(keys.map((k) => [k, ENV_ARG_TYPES[k]]));
	const args = Object.fromEntries(keys.map((k) => [k, DEFAULT_ENV[k]]));
	return { argTypes: withCategory(types, 'Engine'), args };
}

export function wiringArgs(defaultChannels = 2, max = 8) {
	return {
		argTypes: withCategory({ [WIRED_INPUTS]: { control: range(0, max), description: 'Channels cabled in' } }, 'Wiring'),
		args: { [WIRED_INPUTS]: defaultChannels }
	};
}

export function dataArgs(types: ArgTypes, defaults: Record<string, unknown>) {
	return { argTypes: withCategory(types, 'Node data'), args: defaults };
}

/** Merges groups built by the helpers above into one meta `args` / `argTypes`. */
export function compose(...groups: { argTypes: ArgTypes; args: Record<string, unknown> }[]) {
	return {
		argTypes: Object.assign({}, ...groups.map((g) => g.argTypes)) as ArgTypes,
		args: Object.assign({}, ...groups.map((g) => g.args)) as Record<string, unknown>
	};
}

export interface SplitArgs {
	env: MockEnv;
	signal: SignalOptions;
	wiredInputs: number;
	data: Record<string, unknown>;
}

export function splitArgs(args: Record<string, unknown>): SplitArgs {
	const env: Record<string, unknown> = { ...DEFAULT_ENV };
	const signal: Record<string, unknown> = { ...DEFAULT_SIGNAL };
	const data: Record<string, unknown> = {};
	let wiredInputs = 0;
	for (const [key, value] of Object.entries(args)) {
		if (key in DEFAULT_ENV) env[key] = value;
		else if (key in DEFAULT_SIGNAL) signal[key] = value;
		else if (key === WIRED_INPUTS) wiredInputs = Number(value);
		else data[key] = value;
	}
	return { env: env as unknown as MockEnv, signal: signal as unknown as SignalOptions, wiredInputs, data };
}

export { range };
