/** "30s ago", "5m ago", "2h ago", then "M/D HH:MM" past one day. */
export function relativeTime(ms: number): string {
	const diff = Date.now() - ms;
	if (diff < 60_000) return `${Math.max(0, Math.floor(diff / 1000))}s ago`;
	if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m ago`;
	if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}h ago`;
	const d = new Date(ms);
	const HH = d.getHours().toString().padStart(2, '0');
	const MM = d.getMinutes().toString().padStart(2, '0');
	return `${d.getMonth() + 1}/${d.getDate()} ${HH}:${MM}`;
}
