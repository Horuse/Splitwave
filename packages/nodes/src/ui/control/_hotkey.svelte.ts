import { getEnginePort } from '../../engine';

export type HotkeyState = 'Pressed' | 'Released';

/** Holds a global shortcut for as long as the calling component lives; `key`
 * is read reactively, so a rebind swaps the registration. */
export function useGlobalHotkey(key: () => string | undefined, onEvent: (state: HotkeyState) => void): { readonly error: string } {
	const engine = getEnginePort();
	let error = $state('');

	$effect(() => {
		const k = key();
		error = '';
		if (!k) return;

		let live = true;
		// A leftover registration from a previous mount (HMR, node re-render)
		// makes `register` fail with "already registered"; clear it first.
		engine
			.unregisterHotkey(k)
			.catch(() => {})
			.then(() => {
				if (!live) return;
				return engine.registerHotkey(k, (state) => {
					if (live) onEvent(state);
				});
			})
			.catch((err) => {
				if (live) error = String(err);
			});

		return () => {
			live = false;
			engine.unregisterHotkey(k).catch(() => {});
		};
	});

	return {
		get error() {
			return error;
		}
	};
}
