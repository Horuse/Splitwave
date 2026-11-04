import { register, unregister } from '@tauri-apps/plugin-global-shortcut';

export type HotkeyState = 'Pressed' | 'Released';

/** Holds a global shortcut for as long as the calling component lives; `key`
 * is read reactively, so a rebind swaps the registration. */
export function useGlobalHotkey(key: () => string | undefined, onEvent: (state: HotkeyState) => void): { readonly error: string } {
	let error = $state('');

	$effect(() => {
		const k = key();
		error = '';
		if (!k) return;

		let live = true;
		// A leftover registration from a previous mount (HMR, node re-render)
		// makes `register` fail with "already registered"; clear it first.
		unregister(k)
			.catch(() => {})
			.then(() => {
				if (!live) return;
				return register(k, (e) => {
					if (live) onEvent(e.state);
				});
			})
			.catch((err) => {
				if (live) error = String(err);
			});

		return () => {
			live = false;
			unregister(k).catch(() => {});
		};
	});

	return {
		get error() {
			return error;
		}
	};
}
