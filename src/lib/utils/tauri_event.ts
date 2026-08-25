import { onDestroy } from 'svelte';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

/**
 * Component-bound `listen`. Call during component initialisation; the listener
 * is registered as soon as the bridge resolves and is always unregistered on
 * destroy -- including the race where the component unmounts before the
 * mount-time promise settles, which would otherwise leak the listener forever
 * and keep waking the dead component on every event.
 */
export function tauriListen<T>(event: string, handler: (payload: T) => void): void {
	let unlisten: UnlistenFn | undefined;
	const p = listen<T>(event, (e) => {
		if (!unlisten) return;
		handler(e.payload);
	});
	onDestroy(() => {
		p.then(
			(u) => u(),
			(e) => console.warn(`tauriListen(${event}):`, e)
		);
	});
	p.then((u) => (unlisten = u));
}
