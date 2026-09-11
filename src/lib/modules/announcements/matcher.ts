import semver from 'semver';
import type { Announcement, AnnouncementSeverity, EnvironmentContext, ReleaseChannel } from './types';

const SEVERITY_RANK: Record<AnnouncementSeverity, number> = {
	critical: 4,
	warning: 3,
	info: 2,
	success: 1
};

export function cleanVersion(version: string): string {
	return version.replace(/^v/i, '').trim();
}

export function detectChannel(version: string): ReleaseChannel {
	const v = version.toLowerCase();
	if (v.includes('-rc')) return 'rc';
	if (v.includes('-beta')) return 'beta';
	if (v.includes('-alpha')) return 'alpha';
	return 'stable';
}

export function matchesAnnouncement(announcement: Announcement, context: EnvironmentContext, dismissed: Record<string, number> = {}): boolean {
	const filters = announcement.filters;
	const now = context.now ? context.now.getTime() : Date.now();

	if (filters?.expiresAt) {
		const expiry = new Date(filters.expiresAt).getTime();
		if (!Number.isNaN(expiry) && now >= expiry) {
			return false;
		}
	}

	if (filters?.platforms && filters.platforms.length > 0) {
		if (!filters.platforms.includes(context.platform)) {
			return false;
		}
	}

	if (filters?.arch && filters.arch.length > 0) {
		if (!filters.arch.includes(context.arch as 'aarch64' | 'x86_64')) {
			return false;
		}
	}

	if (filters?.channels && filters.channels.length > 0) {
		const currentChannel = detectChannel(context.version);
		if (!filters.channels.includes(currentChannel)) {
			return false;
		}
	}

	if (filters?.versions && filters.versions !== '*' && filters.versions.trim() !== '') {
		const ver = cleanVersion(context.version);
		const validVer = semver.valid(ver) ? ver : semver.coerce(ver)?.version;
		if (!validVer) {
			return false;
		}
		const matches = semver.satisfies(validVer, filters.versions, { includePrerelease: true });
		if (!matches) {
			return false;
		}
	}

	const dismissedAt = dismissed[announcement.id];
	if (dismissedAt) {
		if (announcement.updatedAt) {
			const updatedAt = new Date(announcement.updatedAt).getTime();
			if (!Number.isNaN(updatedAt) && updatedAt > dismissedAt) {
				return true;
			}
		}
		return false;
	}

	return true;
}

export function sortAnnouncements(items: Announcement[]): Announcement[] {
	return [...items].sort((a, b) => {
		const prioDiff = (b.priority ?? 0) - (a.priority ?? 0);
		if (prioDiff !== 0) return prioDiff;

		const sevA = SEVERITY_RANK[a.severity] ?? 0;
		const sevB = SEVERITY_RANK[b.severity] ?? 0;
		return sevB - sevA;
	});
}
