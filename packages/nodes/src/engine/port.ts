import { getContext, setContext } from 'svelte';
import type { NodeTiming } from '../generated/NodeTiming';
import type { Preset, PresetData, PresetKind } from '../presets';
import type { AudioDevice } from './types';

/** Where user presets live; factory presets ship in the package. */
export interface PresetStore {
	list(kind: PresetKind): Promise<Preset[]>;
	create<K extends PresetKind>(kind: K, name: string, data: PresetData<K>): Promise<Preset<K>>;
	remove(id: string): Promise<void>;
	/** Asks the user for a name; null when they cancel. */
	promptName(taken: string[]): Promise<string | null>;
}

/**
 * Everything a node needs from the host. The desktop app backs it with the
 * Tauri engine; a landing page or a story backs it with a fake signal.
 * State is read through getters so a component keeps tracking it: never
 * destructure the port.
 */
export interface EnginePort {
	readonly isRunning: boolean;
	/** Rate the pipeline processes at, Hz. */
	readonly sampleRate: number;
	/** Engine buffer while a pipeline runs; null before the first report. */
	readonly bufferFrames: number | null;
	nodeTiming(nodeId: string): NodeTiming | undefined;
	updateEffect(nodeId: string, patch: Record<string, unknown>): Promise<void>;
	/** Subscribes to an engine event. Called during component init it also unsubscribes on destroy. */
	listen<T>(event: string, handler: (payload: T) => void): () => void;
	readonly outputDevices: AudioDevice[];
	refreshOutputDevices(): Promise<void>;
	playCue(deviceId: string, muted: boolean, gain: number, beep?: boolean): Promise<void>;
	readonly presets: PresetStore;
}

const KEY = Symbol('engine-port');

export function setEnginePort(port: EnginePort): void {
	setContext(KEY, port);
}

/** Throws when no host provided a port: a node rendered without an engine is a wiring bug, not a state to paper over. */
export function getEnginePort(): EnginePort {
	const port = getContext<EnginePort | undefined>(KEY);
	if (!port) throw new Error('EnginePort missing: wrap nodes in a host that calls setEnginePort()');
	return port;
}
