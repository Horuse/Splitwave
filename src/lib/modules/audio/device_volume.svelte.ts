import { methods } from './methods';
import type { DeviceKind } from './types';

const POLL_MS = 1000;
// A poll answered from before our write would snap the slider back.
const SETTLE_MS = 500;

export interface DeviceVolumeState {
	/** `null` while unknown or unsupported. */
	readonly scalar: number | null;
	/** Device attenuation in dB; `null` when the backend can't report it. */
	readonly db: number | null;
	/** The device exposes no software-settable volume. */
	readonly unsupported: boolean;
	set(scalar: number): Promise<void>;
}

/**
 * Tracks one device's hardware volume, polling so changes made outside the app
 * show up. The interface is event-shaped so native change notifications can
 * replace the polling without touching callers.
 */
export function deviceVolume(kind: DeviceKind, deviceId: () => string | null): DeviceVolumeState {
	let scalar = $state<number | null>(null);
	let db = $state<number | null>(null);
	let unsupported = $state(false);
	let settleUntil = 0;

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
			return;
		}
		void load(id);
		const timer = setInterval(() => {
			if (document.hidden || Date.now() < settleUntil) return;
			void load(id);
		}, POLL_MS);
		return () => clearInterval(timer);
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
		async set(next: number) {
			const id = deviceId();
			if (!id || unsupported) return;
			const clamped = Math.max(0, Math.min(1, next));
			settleUntil = Date.now() + SETTLE_MS;
			scalar = clamped;
			try {
				await methods.setDeviceVolume(kind, id, clamped);
			} catch {
				unsupported = true;
				return;
			}
			await load(id, true);
			settleUntil = Date.now() + SETTLE_MS;
		}
	};
}
