import { t } from '$lib/modules/i18n';

/** Localized relative time ("30s ago" / "5 分钟前"), then "M/D HH:MM" past one day. */
export function relativeTime(ms: number): string {
	const diff = Date.now() - ms;
	if (diff < 60_000) return t('time.secondsAgo', { n: Math.max(0, Math.floor(diff / 1000)) });
	if (diff < 3_600_000) return t('time.minutesAgo', { n: Math.floor(diff / 60_000) });
	if (diff < 86_400_000) return t('time.hoursAgo', { n: Math.floor(diff / 3_600_000) });
	const d = new Date(ms);
	const HH = d.getHours().toString().padStart(2, '0');
	const MM = d.getMinutes().toString().padStart(2, '0');
	return `${d.getMonth() + 1}/${d.getDate()} ${HH}:${MM}`;
}
