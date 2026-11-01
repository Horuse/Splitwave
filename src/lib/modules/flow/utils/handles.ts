// Every socket is named `<signal>:<port>`. This module is the only place that
// builds or reads that format; the engine parses the same grammar in
// `src-tauri/src/audio/port.rs`.

export type Signal = 'audio' | 'control';

export type AudioPort =
	| { kind: 'main' }
	| { kind: 'channel'; ch: number }
	| { kind: 'stereo'; ch: number }
	| { kind: 'peer'; peer: string; ch: number | null }
	| { kind: 'sidechain' }
	| { kind: 'branch'; branch: number; port: AudioPort };

export type Port = { signal: 'audio'; port: AudioPort } | { signal: 'control'; name: string };

/** A node's whole bus. */
export const MAIN_HANDLE = 'audio:main';
export const SIDECHAIN_HANDLE = 'audio:sidechain';

/** One channel, 1-based. */
export function channelHandle(ch: number): string {
	return `audio:ch${ch}`;
}

/** A WebRTC peer's mix, or one of its channels counted from zero. */
export function peerHandle(peer: string, ch?: number): string {
	return ch === undefined ? `audio:peer:${peer}` : `audio:peer:${peer}:${ch}`;
}

/** Channel `ch` of output `branch` on an If or Switch, both 1-based. */
export function branchHandle(branch: number, ch: number): string {
	return `audio:branch${branch}:ch${ch}`;
}

export function controlHandle(name: string): string {
	return `control:${name}`;
}

export const CONTROL_IN = controlHandle('in');
export const CONTROL_OUT = controlHandle('out');
/** High runs the node, low bypasses it. */
export const POWER_HANDLE = controlHandle('power');
/** The inputs an If's logic operator reads. */
export const A_HANDLE = controlHandle('a');
export const B_HANDLE = controlHandle('b');

/** Rising edge picks output `k` of a Switch. */
export function selectHandle(k: number): string {
	return controlHandle(`select${k}`);
}

const INDEX = /^[1-9]\d*$/;

function parseAudioPort(port: string): AudioPort | null {
	if (port === 'main') return { kind: 'main' };
	if (port === 'sidechain') return { kind: 'sidechain' };
	const branch = /^branch([^:]*):(.*)$/.exec(port);
	if (branch) {
		const inner = parseAudioPort(branch[2]);
		if (!INDEX.test(branch[1]) || !inner || inner.kind === 'branch') return null;
		return { kind: 'branch', branch: Number(branch[1]), port: inner };
	}
	const peer = /^peer:([^:]+)(?::(\d+))?$/.exec(port);
	if (peer) return { kind: 'peer', peer: peer[1], ch: peer[2] === undefined ? null : Number(peer[2]) };
	const indexed = /^(ch|st)(.*)$/.exec(port);
	if (indexed && INDEX.test(indexed[2])) {
		return { kind: indexed[1] === 'ch' ? 'channel' : 'stereo', ch: Number(indexed[2]) };
	}
	return null;
}

export function parsePort(handle: string | null | undefined): Port | null {
	if (!handle) return null;
	const sep = handle.indexOf(':');
	if (sep < 0) return null;
	const rest = handle.slice(sep + 1);
	switch (handle.slice(0, sep)) {
		case 'audio': {
			const port = parseAudioPort(rest);
			return port && { signal: 'audio', port };
		}
		case 'control':
			return rest ? { signal: 'control', name: rest } : null;
		default:
			return null;
	}
}

export function signalOf(handle: string | null | undefined): Signal | null {
	return parsePort(handle)?.signal ?? null;
}

/** The 1-based channel a `channelHandle` addresses, or null for any other port. */
export function channelOf(handle: string | null | undefined): number | null {
	const p = parsePort(handle);
	return p?.signal === 'audio' && p.port.kind === 'channel' ? p.port.ch : null;
}

/** A cable joins two sockets of the same signal; audio never lands on a control input. */
export function canConnect(source: string | null | undefined, target: string | null | undefined): boolean {
	const signal = signalOf(source);
	return signal !== null && signal === signalOf(target);
}
