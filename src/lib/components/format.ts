/** Human-readable transfer rate, e.g. `12.3 kB/s`, `1.4 MB/s`. */
export function formatRate(bytesPerSec: number): string {
	if (bytesPerSec < 1) return '0 B/s';
	if (bytesPerSec < 1024) return `${Math.round(bytesPerSec)} B/s`;
	if (bytesPerSec < 1024 * 1024) return `${(bytesPerSec / 1024).toFixed(1)} kB/s`;
	return `${(bytesPerSec / (1024 * 1024)).toFixed(2)} MB/s`;
}
