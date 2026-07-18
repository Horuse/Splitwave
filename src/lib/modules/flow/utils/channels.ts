// Curated categorical palette — vibrant mid-tones that stay legible on both
// light and dark backgrounds. ch1/ch2 keep the DAW-familiar red/blue; the rest
// are distinct hues, cycling for wide channel counts.
const CHANNEL_PALETTE = [
	'#ef4444', // red   (L)
	'#3b82f6', // blue  (R)
	'#22c55e', // green
	'#f59e0b', // amber
	'#a855f7', // purple
	'#ec4899', // pink
	'#14b8a6', // teal
	'#f97316', // orange
	'#8b5cf6', // violet
	'#06b6d4', // cyan
	'#84cc16', // lime
	'#e11d48', // rose
	'#0ea5e9', // sky
	'#d946ef', // fuchsia
	'#10b981', // emerald
	'#eab308' // yellow
];

export function channelColor(index: number): string {
	return CHANNEL_PALETTE[index % CHANNEL_PALETTE.length];
}

export function channelLabel(index: number, total: number): string {
	if (total === 2) return index === 0 ? 'L' : 'R';
	return `ch${index + 1}`;
}

// Darken a #rrggbb hex toward black by `factor` (0..1).
export function darken(hex: string, factor = 0.6): string {
	const n = parseInt(hex.slice(1), 16);
	const r = Math.round(((n >> 16) & 255) * factor);
	const g = Math.round(((n >> 8) & 255) * factor);
	const b = Math.round((n & 255) * factor);
	return `#${((1 << 24) | (r << 16) | (g << 8) | b).toString(16).slice(1)}`;
}

// The `.handle` class forces a neutral background/border with `!important`;
// inline styles must also use `!important` to tint a handle. Border is a darker
// shade of the fill, derived automatically.
export function handleStyle(color: string): string {
	const edge = darken(color);
	return `background:${color} !important;border:1px solid ${edge} !important;--tw-ring-color:${edge}`;
}

// Nudge a handle from the padded content edge onto the node's outer border
// (node padding is 1rem). Avoids the negative-margin "overhang" rows, which
// desynced the visible dot from its clickable connection target.
export function handleEdgeStyle(color: string, side: 'source' | 'target'): string {
	const edge = side === 'source' ? 'right:-1rem !important;' : 'left:-1rem !important;';
	return `${handleStyle(color)};${edge}`;
}

// A stereo group is stored as the 1-based lower channel it pairs with the next.

export function groupLowerOf(groups: number[], ch1: number): number | null {
	for (const g of groups) if (g === ch1 || g + 1 === ch1) return g;
	return null;
}

export function toggleGroup(groups: number[], lower: number): number[] {
	if (groups.includes(lower)) return groups.filter((g) => g !== lower);
	// Drop any group overlapping this pair, then add it.
	return [...groups.filter((g) => g !== lower - 1 && g !== lower + 1), lower].sort((a, b) => a - b);
}
