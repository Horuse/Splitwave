import { emit } from '@tauri-apps/api/event';
import { startSignal as startDemoSignal, type SignalTargets, type SignalOptions } from '@splitwave/nodes/engine';

export { DEFAULT_SIGNAL, SIGNAL_SHAPES, type SignalOptions, type SignalShape, type SignalTargets } from '@splitwave/nodes/engine';

/** Feeds the synthetic stream through the mocked Tauri event bus the app port listens on. */
export function startSignal(targets: SignalTargets, opts: SignalOptions): () => void {
	return startDemoSignal(targets, opts, (event, payload) => void emit(event, payload));
}
