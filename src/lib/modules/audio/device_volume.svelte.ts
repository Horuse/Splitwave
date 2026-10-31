import type { UnlistenFn } from '@tauri-apps/api/event';
import { methods } from './methods';
import type { DeviceKind } from './types';

// An `audio://device_volume` event for a write we just made can land after the
// user has already dragged the slider further; ignore that echo.
const SETTLE_MS = 200;

export interface DeviceVolumeState {
	/** `null` while unknown or unsupported. */
	readonly scalar: number | null;
	/** Device attenuation in dB; `null` when the backend can't report it. */
	readonly db: number | null;
	/** The device exposes no software-settable volume. */
	readonly unsupported: boolean;
	/** The OS reports no volume changes for this device, so the value can go stale. */
	readonly unsynced: boolean;
	set(scalar: number): Promise<void>;
}

/**
 * Tracks one device's hardware volume, following changes made outside the app
 * through the backend's native listener.
 */
export function deviceVolume(kind: DeviceKind, deviceId: () => string | null): DeviceVolumeState {
	let scalar = $state<number | null>(null);
	let db = $state<number | null>(null);
	let unsupported = $state(false);
	let unsynced = $state(false);
	let settleUntil = 0;

	// Concurrent writes can reach the device out of order and leave a stale
	// value, so one runs at a time and only the latest queued value is sent.
	let queued: { id: string; scalar: number } | null = null;
	let writer: Promise<void> | null = null;

	async function flush() {
		while (queued) {
			const { id, scalar: value } = queued;
			queued = null;
			try {
				await methods.setDeviceVolume(kind, id, value);
			} catch {
				queued = null;
				unsupported = true;
				return;
			}
			if (queued) continue;
			await load(id, true);
			settleUntil = Date.now() + SETTLE_MS;
		}
	}

	async function load(id: string, force = false) {
		try {
			const v = await methods.getDeviceVolume(kind, id);
			if (id !== deviceId() || (!force && Date.now() < settleUntil)) return;
			unsupported = v === null;
			scalar = v?.scalar ?? null;
			db = v?.db ?? null;
		} catch {
			unsupported = true;
			scalar = null;
			db = null;
		}
	}

	$effect(() => {
		const id = deviceId();
		if (!id) {
			scalar = null;
			db = null;
			unsupported = false;
			unsynced = false;
			return;
		}
		let unlisten: UnlistenFn | undefined;
		let stopped = false;
		void load(id);
		const watched = methods
			.watchDeviceVolume(kind, id)
			.then(async () => {
				unsynced = false;
				const un = await methods.onDeviceVolume((e) => {
					if (e.kind !== kind || e.name !== id || Date.now() < settleUntil) return;
					unsupported = false;
					scalar = e.scalar;
					db = e.db;
				});
				if (stopped) un();
				else unlisten = un;
			})
			.then(
				() => true,
				() => {
					unsynced = true;
					return false;
				}
			);
		return () => {
			stopped = true;
			unlisten?.();
			// Commands run concurrently in the backend; an unwatch that overtakes
			// its watch finds nothing and leaks the listener, and one after a failed
			// watch would drop another node's reference.
			void watched.then((ok) => {
				if (ok) void methods.unwatchDeviceVolume(kind, id);
			});
		};
	});

	return {
		get scalar() {
			return scalar;
		},
		get db() {
			return db;
		},
		get unsupported() {
			return unsupported;
		},
		get unsynced() {
			return unsynced;
		},
		async set(next: number) {
			const id = deviceId();
			if (!id || unsupported) return;
			const clamped = Math.max(0, Math.min(1, next));
			settleUntil = Date.now() + SETTLE_MS;
			scalar = clamped;
			queued = { id, scalar: clamped };
			writer ??= flush().finally(() => (writer = null));
			return writer;
		}
	};
}
