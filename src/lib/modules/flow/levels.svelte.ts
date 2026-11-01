import { SvelteMap } from 'svelte/reactivity';
import { tauriListen } from '$lib/utils/tauri_event';
import { parsePort } from './utils/handles';

interface MeterTick {
	nodeId: string;
	peaks: number[];
}

// Linear peaks per node and channel, as the engine's meter tick reports them.
const peaks = new SvelteMap<string, number[]>();

export function recordLevels(nodeId: string, linearPeaks: number[]): void {
	peaks.set(nodeId, linearPeaks);
}

/** Feeds cable levels from the engine for as long as the calling component lives. */
export function listenCableLevels(): void {
	tauriListen<MeterTick>('audio://meter', (p) => recordLevels(p.nodeId, p.peaks));
}

/** Linear, slightly lifted so speech-level audio is still visible: a dB scale
 * would draw most program material near full width all the time. */
function normalize(linear: number): number {
	return Math.min(1, Math.max(0, linear) ** 0.7);
}

/** How loud the audio on a cable is, 0..1. A channel cable reads that channel;
 * any other reads the loudest channel of its source. A branch output is metered
 * on the gate the engine expands it into (`branch_gate_id` in `graph.rs`). */
export function cableLevel(source: string, sourceHandle: string | null | undefined): number {
	const p = parsePort(sourceHandle);
	if (p?.signal !== 'audio') return 0;
	let node = source;
	let port = p.port;
	if (port.kind === 'branch') {
		node = `${source}#b${port.branch}`;
		port = port.port;
	}
	const channels = peaks.get(node);
	if (!channels || channels.length === 0) return 0;
	const linear = port.kind === 'channel' ? (channels[port.ch - 1] ?? 0) : Math.max(...channels);
	return normalize(linear);
}
