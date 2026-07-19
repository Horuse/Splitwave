import { LazyStore } from '@tauri-apps/plugin-store';
import { createId } from '@paralleldrive/cuid2';
import type { Preset, PresetData, PresetKind } from './types';
import { FACTORY_PRESETS } from './factory';
import { PRESET_VERSION, isOutdated } from './version';

const STORE_FILE = 'presets.json';
const KEY_PREFIX = 'preset:';
const store = new LazyStore(STORE_FILE);

async function userPresets(): Promise<Preset[]> {
	const entries = await store.entries<Preset>();
	return entries
		.filter(([k]) => k.startsWith(KEY_PREFIX))
		.map(([, v]) => v)
		.filter((p) => !isOutdated(p));
}

export const methods = {
	/** Factory presets first, then the user's newest-first. */
	async list(kind: PresetKind): Promise<Preset[]> {
		const mine = (await userPresets())
			.filter((p) => p.kind === kind)
			.sort((a, b) => b.createdAt - a.createdAt);
		return [...FACTORY_PRESETS.filter((p) => p.kind === kind), ...mine];
	},

	async create<K extends PresetKind>(kind: K, name: string, data: PresetData<K>): Promise<Preset<K>> {
		const preset: Preset<K> = {
			id: createId(),
			kind,
			name,
			data,
			createdAt: Date.now(),
			version: PRESET_VERSION
		};
		await store.set(KEY_PREFIX + preset.id, preset);
		await store.save();
		return preset;
	},

	async remove(id: string): Promise<void> {
		await store.delete(KEY_PREFIX + id);
		await store.save();
	}
};
