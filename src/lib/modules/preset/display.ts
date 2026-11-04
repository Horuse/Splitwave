import { t } from '$lib/modules/i18n';
import type { Preset } from './types';

/** Factory presets ship with an English name that doubles as their identity, so
 * the localized label is looked up by slug instead of replacing `name` — a
 * stored preset keeps matching across languages. */
export function presetDisplayName(preset: Preset): string {
	if (!preset.builtin) return preset.name;
	const slug = preset.name.replace(/[^A-Za-z0-9]+/g, '');
	const key = `presetName.${preset.kind}.${slug}`;
	const label = t(key);
	// A factory preset added without its translation would otherwise render the
	// raw key; its English name is a better fallback.
	return label === key ? preset.name : label;
}
