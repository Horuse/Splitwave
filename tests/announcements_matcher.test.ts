import { describe, expect, it } from 'bun:test';
import { detectChannel, matchesAnnouncement, sortAnnouncements } from '../src/lib/modules/announcements/matcher';
import { parseAnnouncementsPayload, type Announcement, type EnvironmentContext } from '../src/lib/modules/announcements/types';

describe('detectChannel', () => {
	it('detects rc channel', () => {
		expect(detectChannel('1.3.0-rc.1')).toBe('rc');
		expect(detectChannel('v1.2.1-rc.2')).toBe('rc');
	});

	it('detects beta channel', () => {
		expect(detectChannel('1.3.0-beta.1')).toBe('beta');
		expect(detectChannel('v1.0.0-beta')).toBe('beta');
	});

	it('detects alpha channel', () => {
		expect(detectChannel('1.0.0-alpha.5')).toBe('alpha');
	});

	it('detects stable channel', () => {
		expect(detectChannel('1.2.0')).toBe('stable');
		expect(detectChannel('v1.3.0')).toBe('stable');
	});
});

describe('matchesAnnouncement', () => {
	const defaultContext: EnvironmentContext = {
		version: '1.3.0-rc.1',
		platform: 'macos',
		arch: 'aarch64',
		now: new Date('2026-09-11T20:00:00Z')
	};

	it('matches wildcard version and all platforms', () => {
		const item: Announcement = {
			id: 'general-notice',
			type: 'banner',
			severity: 'info',
			title: 'Hello',
			message: 'World'
		};
		expect(matchesAnnouncement(item, defaultContext)).toBe(true);
	});

	it('matches semver range including pre-release', () => {
		const item: Announcement = {
			id: 'rc-notice',
			type: 'banner',
			severity: 'warning',
			title: 'RC testing',
			message: 'Test message',
			filters: {
				versions: '>=1.3.0-rc.0 <1.3.0'
			}
		};
		expect(matchesAnnouncement(item, defaultContext)).toBe(true);

		// Non-matching version
		expect(matchesAnnouncement(item, { ...defaultContext, version: '1.2.0' })).toBe(false);
	});

	it('filters by exact version', () => {
		const item: Announcement = {
			id: 'exact-fix',
			type: 'banner',
			severity: 'critical',
			title: 'Notice for 1.2.1',
			message: 'Update info',
			filters: {
				versions: '1.2.1'
			}
		};
		expect(matchesAnnouncement(item, { ...defaultContext, version: '1.2.1' })).toBe(true);
		expect(matchesAnnouncement(item, { ...defaultContext, version: '1.2.0' })).toBe(false);
	});

	it('filters by platform', () => {
		const macOnly: Announcement = {
			id: 'mac-only',
			type: 'banner',
			severity: 'info',
			title: 'Mac notice',
			message: 'Info',
			filters: {
				platforms: ['macos']
			}
		};
		expect(matchesAnnouncement(macOnly, defaultContext)).toBe(true);
		expect(matchesAnnouncement(macOnly, { ...defaultContext, platform: 'windows' })).toBe(false);
	});

	it('filters by pre-release channel', () => {
		const rcOnly: Announcement = {
			id: 'rc-survey',
			type: 'banner',
			severity: 'info',
			title: 'RC Survey',
			message: 'Survey',
			filters: {
				channels: ['rc']
			}
		};
		expect(matchesAnnouncement(rcOnly, defaultContext)).toBe(true);
		expect(matchesAnnouncement(rcOnly, { ...defaultContext, version: '1.4.0-rc.2' })).toBe(true);
		expect(matchesAnnouncement(rcOnly, { ...defaultContext, version: '1.3.0-beta.1' })).toBe(false);
		expect(matchesAnnouncement(rcOnly, { ...defaultContext, version: '1.2.0' })).toBe(false);
	});

	it('respects expiry timestamp', () => {
		const expired: Announcement = {
			id: 'expired-promo',
			type: 'banner',
			severity: 'info',
			title: 'Old promo',
			message: 'Expired',
			filters: {
				expiresAt: '2026-09-01T00:00:00Z'
			}
		};
		expect(matchesAnnouncement(expired, defaultContext)).toBe(false);
	});

	it('respects dismissal and updatedAt', () => {
		const item: Announcement = {
			id: 'dismiss-test',
			type: 'banner',
			severity: 'info',
			title: 'Dismiss test',
			message: 'Test'
		};

		const dismissed = {
			'dismiss-test': 1000
		};

		// Dismissed and not updated
		expect(matchesAnnouncement(item, defaultContext, dismissed)).toBe(false);

		// Updated after dismissal
		const updatedItem: Announcement = {
			...item,
			updatedAt: new Date(2000).toISOString()
		};
		expect(matchesAnnouncement(updatedItem, defaultContext, dismissed)).toBe(true);
	});
});

describe('sortAnnouncements', () => {
	it('sorts by priority first, then severity', () => {
		const items: Announcement[] = [
			{ id: '1', priority: 10, severity: 'info', type: 'banner', title: '1', message: '' },
			{ id: '2', priority: 50, severity: 'warning', type: 'banner', title: '2', message: '' },
			{ id: '3', priority: 50, severity: 'critical', type: 'banner', title: '3', message: '' },
			{ id: '4', priority: 0, severity: 'critical', type: 'banner', title: '4', message: '' }
		];

		const sorted = sortAnnouncements(items);
		expect(sorted.map((s) => s.id)).toEqual(['3', '2', '1', '4']);
	});
});

describe('parseAnnouncementsPayload', () => {
	it('keeps valid announcements and rejects malformed entries', () => {
		const payload = parseAnnouncementsPayload({
			announcements: [
				{ id: 'valid', type: 'banner', severity: 'info', title: 'Title', message: 'Message' },
				{ id: 'invalid-type', type: 'toast', severity: 'info', title: 'Title', message: 'Message' },
				{ id: 'missing-message', type: 'banner', severity: 'info', title: 'Title' }
			]
		});

		expect(payload?.announcements.map((item) => item.id)).toEqual(['valid']);
	});

	it('keeps only the first announcement with a duplicate id', () => {
		const payload = parseAnnouncementsPayload({
			announcements: [
				{ id: 'same', type: 'banner', severity: 'info', title: 'First', message: 'Message' },
				{ id: 'same', type: 'modal', severity: 'warning', title: 'Second', message: 'Message' }
			]
		});

		expect(payload?.announcements).toHaveLength(1);
		expect(payload?.announcements[0]?.title).toBe('First');
	});

	it('rejects an invalid payload envelope', () => {
		expect(parseAnnouncementsPayload({ announcements: null })).toBeNull();
		expect(parseAnnouncementsPayload([])).toBeNull();
	});
});
