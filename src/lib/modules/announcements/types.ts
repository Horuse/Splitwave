export type AnnouncementType = 'banner' | 'modal' | 'both';

export type AnnouncementSeverity = 'info' | 'warning' | 'critical' | 'success';

export type ReleaseChannel = 'stable' | 'rc' | 'beta' | 'alpha';

export interface AnnouncementAction {
	label: string;
	url?: string;
	dismissOnClick?: boolean;
}

export interface AnnouncementFilters {
	versions?: string;
	platforms?: Array<'macos' | 'windows' | 'linux'>;
	channels?: ReleaseChannel[];
	arch?: Array<'aarch64' | 'x86_64'>;
	expiresAt?: string;
}

export interface Announcement {
	id: string;
	priority?: number;
	updatedAt?: string;
	type: AnnouncementType;
	severity: AnnouncementSeverity;
	badge?: string;
	title: string;
	message: string;
	markdown?: string;
	action?: AnnouncementAction;
	dismissible?: boolean;
	filters?: AnnouncementFilters;
}

export interface AnnouncementsPayload {
	announcements: Announcement[];
}

export interface EnvironmentContext {
	version: string;
	platform: 'macos' | 'windows' | 'linux';
	arch: string;
	now?: Date;
}

const TYPES: AnnouncementType[] = ['banner', 'modal', 'both'];
const SEVERITIES: AnnouncementSeverity[] = ['info', 'warning', 'critical', 'success'];
const CHANNELS: ReleaseChannel[] = ['stable', 'rc', 'beta', 'alpha'];
const PLATFORMS = ['macos', 'windows', 'linux'] as const;
const ARCHITECTURES = ['aarch64', 'x86_64'] as const;

function isRecord(value: unknown): value is Record<string, unknown> {
	return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function optionalString(value: unknown): value is string | undefined {
	return value === undefined || typeof value === 'string';
}

function stringArrayOf<T extends string>(value: unknown, allowed: readonly T[]): value is T[] | undefined {
	return value === undefined || (Array.isArray(value) && value.every((item) => typeof item === 'string' && allowed.includes(item as T)));
}

export function isAnnouncement(value: unknown): value is Announcement {
	if (!isRecord(value)) return false;
	if (
		typeof value.id !== 'string' ||
		value.id.trim() === '' ||
		!TYPES.includes(value.type as AnnouncementType) ||
		!SEVERITIES.includes(value.severity as AnnouncementSeverity) ||
		typeof value.title !== 'string' ||
		typeof value.message !== 'string' ||
		(value.priority !== undefined && (typeof value.priority !== 'number' || !Number.isFinite(value.priority))) ||
		(value.dismissible !== undefined && typeof value.dismissible !== 'boolean') ||
		!optionalString(value.badge) ||
		!optionalString(value.markdown) ||
		!optionalString(value.updatedAt)
	) {
		return false;
	}

	if (value.action !== undefined) {
		if (!isRecord(value.action) || typeof value.action.label !== 'string' || value.action.label.trim() === '' || !optionalString(value.action.url))
			return false;
		if (value.action.dismissOnClick !== undefined && typeof value.action.dismissOnClick !== 'boolean') return false;
	}

	if (value.filters !== undefined) {
		if (!isRecord(value.filters)) return false;
		if (!optionalString(value.filters.versions) || !optionalString(value.filters.expiresAt)) return false;
		if (!stringArrayOf(value.filters.platforms, PLATFORMS)) return false;
		if (!stringArrayOf(value.filters.channels, CHANNELS)) return false;
		if (!stringArrayOf(value.filters.arch, ARCHITECTURES)) return false;
	}

	return true;
}

export function parseAnnouncementsPayload(value: unknown): AnnouncementsPayload | null {
	if (!isRecord(value) || !Array.isArray(value.announcements)) return null;
	const seen = new Set<string>();
	return {
		announcements: value.announcements.filter((item): item is Announcement => {
			if (!isAnnouncement(item) || seen.has(item.id)) return false;
			seen.add(item.id);
			return true;
		})
	};
}
