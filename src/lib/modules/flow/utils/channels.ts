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
