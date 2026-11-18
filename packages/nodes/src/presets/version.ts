import type { Preset } from './types';

/** Bumped when a stored preset's parameter shape stops matching its effect.
 * Old presets are hidden rather than coerced -- a missing field would silently
 * apply the effect's default in its place. */
export const PRESET_VERSION = 1;

export function versionOf(preset: Preset): number {
	return typeof preset.version === 'number' ? preset.version : 0;
}

export function isOutdated(preset: Preset): boolean {
	return versionOf(preset) < PRESET_VERSION;
}
