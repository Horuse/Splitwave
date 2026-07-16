// Per-channel handle colors. L/R fixed to the DAW-familiar red/blue; the rest
// walk the hue wheel by the golden angle so adjacent channels stay distinct.
export function channelColor(index: number): string {
	if (index === 0) return '#ef4444';
	if (index === 1) return '#3b82f6';
	const hue = (index * 137.508) % 360;
	return `hsl(${hue} 70% 55%)`;
}

export function channelLabel(index: number, total: number): string {
	if (total === 2) return index === 0 ? 'L' : 'R';
	return `ch${index + 1}`;
}
