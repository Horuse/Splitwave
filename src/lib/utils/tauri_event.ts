import { onDestroy } from 'svelte';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

/**
 * Component-bound `listen`. Safely subscribes to Tauri events with reliable teardown.
 * Unregisters automatically on destroy when called during component initialization,
 * and also returns an explicit cleanup function for use in onMount or manual teardown.
 */
export function tauriListen<T>(event: string, handler: (payload: T) => void): () => void {
	let unlistenFn: UnlistenFn | null = null;
	let disposed = false;

	listen<T>(event, (e) => {
		if (disposed) return;
		handler(e.payload);
	})
		.then((fn) => {
			if (disposed) {
				fn();
			} else {
				unlistenFn = fn;
			}
		})
		.catch((err) => {
			console.warn(`tauriListen(${event}) setup failed:`, err);
		});

	const cleanup = () => {
		if (disposed) return;
		disposed = true;
		if (unlistenFn) {
			unlistenFn();
			unlistenFn = null;
		}
	};

	try {
		onDestroy(cleanup);
	} catch {
		// Lifecycle called outside component initialization (e.g. inside onMount);
		// caller should retain and call the returned cleanup function.
	}

	return cleanup;
}
