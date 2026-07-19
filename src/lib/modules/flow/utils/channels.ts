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

// Matches the connector pin on the cable end, so the two read as one seated joint.
const PIN_SHAPE =
	'width:5px !important;height:10px !important;border-radius:2px !important;';

// Nudge a handle from the padded content edge onto the node's outer border
// (node padding is 1rem). Avoids the negative-margin "overhang" rows, which
// desynced the visible dot from its clickable connection target.
export function handleEdgeStyle(color: string, side: 'source' | 'target'): string {
	const edge = side === 'source' ? 'right:-1rem !important;' : 'left:-1rem !important;';
	return `${handleStyle(color)};${PIN_SHAPE}${edge}`;
}

export interface Slot {
	id: string;
	ch: number;
	occupied: boolean;
}

export function parseHandle(handle: string): number | null {
	const m = /^ch(\d+)$/.exec(handle);
	return m ? Number(m[1]) : null;
}

// A removed cable frees its slot in place; renumbering would reroute live audio.
export function deriveSlots(
	occupiedHandles: string[],
	trailing: boolean,
	max = Infinity,
	min = 0
): Slot[] {
	const taken = new Set<number>();
	for (const h of occupiedHandles) {
		const ch = parseHandle(h);
		if (ch !== null) taken.add(ch);
	}

	const end = Math.min(Math.max(taken.size === 0 ? 0 : Math.max(...taken), min), max);
	const slots: Slot[] = [];
	for (let ch = 1; ch <= end; ch++) {
		slots.push({ id: `ch${ch}`, ch, occupied: taken.has(ch) });
	}
	if (trailing && end < max) {
		slots.push({ id: `ch${end + 1}`, ch: end + 1, occupied: false });
	}
	return slots;
}

export function freeRunFrom(occupiedHandles: string[], start: number, count: number): number[] {
	const taken = new Set<number>();
	for (const h of occupiedHandles) {
		const ch = parseHandle(h);
		if (ch !== null) taken.add(ch);
	}
	const run: number[] = [];
	for (let ch = start; run.length < count; ch++) {
		if (!taken.has(ch)) run.push(ch);
	}
	return run;
}
