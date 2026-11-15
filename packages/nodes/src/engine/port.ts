import { getContext, setContext } from 'svelte';
import type { NodeTiming } from '../generated/NodeTiming';

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
