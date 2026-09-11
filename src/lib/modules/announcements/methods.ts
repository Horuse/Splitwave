import { arch, platform } from '@tauri-apps/plugin-os';
import { openUrl } from '@tauri-apps/plugin-opener';
import { getCachedAppInfo, loadAppInfo } from '$lib/modules/app_info';
import { modalManager } from '$lib/modules/overlay/modal';
import { announcementStore } from './stores.svelte';
import { matchesAnnouncement, sortAnnouncements } from './matcher';
import { parseAnnouncementsPayload, type Announcement, type EnvironmentContext } from './types';
import AnnouncementModal from './ui/announcement_modal.svelte';

export const DEFAULT_ANNOUNCEMENTS_URL = 'https://raw.githubusercontent.com/Horuse/Splitwave/main/announcements.json';

async function getEnvironmentContext(): Promise<EnvironmentContext | null> {
	let info = getCachedAppInfo();
	if (!info) {
		try {
			info = await loadAppInfo();
		} catch {}
	}

	const os = platform();
	if (!info || (os !== 'macos' && os !== 'windows' && os !== 'linux')) return null;

	return {
		version: info.appVersion,
		platform: os,
		arch: arch(),
		now: new Date()
	};
}

export async function fetchAnnouncements(url: string = DEFAULT_ANNOUNCEMENTS_URL): Promise<void> {
	if (announcementStore.isLoading) return;
	announcementStore.isLoading = true;

	try {
		const context = await getEnvironmentContext();
		if (!context) return;
		const response = await fetch(url, {
			cache: 'no-cache',
			headers: { Accept: 'application/json' }
		});

		if (!response.ok) return;

		const data = parseAnnouncementsPayload(await response.json());
		if (!data) return;

		const matching = data.announcements.filter((item) => matchesAnnouncement(item, context, announcementStore.dismissed));

		const sorted = sortAnnouncements(matching);

		const banners = sorted.filter((item) => item.type === 'banner' || item.type === 'both');
		const modals = sorted.filter((item) => item.type === 'modal' || item.type === 'both');

		announcementStore.bannerQueue = banners;
		if (announcementStore.isModalActive && announcementStore.modalQueue[0]) {
			const current = announcementStore.modalQueue[0];
			announcementStore.modalQueue = [current, ...modals.filter((item) => item.id !== current.id)];
		} else {
			announcementStore.modalQueue = modals;
		}

		if (modals.length > 0 && !announcementStore.isModalActive) {
			runModalQueue().catch(() => {});
		}
	} catch {
	} finally {
		announcementStore.isLoading = false;
	}
}

export async function runModalQueue(): Promise<void> {
	if (announcementStore.modalQueue.length === 0) {
		announcementStore.isModalActive = false;
		return;
	}

	announcementStore.isModalActive = true;
	const current = announcementStore.modalQueue[0];

	try {
		await modalManager.open(current.title, AnnouncementModal, {
			announcement: current,
			totalModals: announcementStore.modalQueue.length,
			canClose: true,
			onClose: () => dismissAnnouncement(current)
		});
	} catch {
	} finally {
		announcementStore.modalQueue = announcementStore.modalQueue.filter((m) => m.id !== current.id);
		if (announcementStore.modalQueue.length > 0) {
			queueMicrotask(() => runModalQueue().catch(() => {}));
		} else {
			announcementStore.isModalActive = false;
		}
	}
}

export function openAnnouncementDetails(announcement: Announcement): void {
	modalManager
		.open(announcement.title, AnnouncementModal, {
			announcement,
			totalModals: 1,
			canClose: true,
			onClose: () => dismissAnnouncement(announcement)
		})
		.catch(() => {});
}

export async function handleAnnouncementAction(announcement: Announcement): Promise<void> {
	if (announcement.action?.url) {
		const url = new URL(announcement.action.url);
		if (url.protocol !== 'https:' && url.protocol !== 'http:') throw new Error('Unsupported announcement URL');
		await openUrl(url.toString());
	}
	if (announcement.action?.dismissOnClick) {
		announcementStore.dismiss(announcement.id);
	}
}

export function dismissAnnouncement(announcement: Announcement): void {
	if (announcement.dismissible !== false) announcementStore.dismiss(announcement.id);
}

let initialized = false;
export function initAnnouncements(): void {
	if (initialized) return;
	initialized = true;
	fetchAnnouncements().catch(() => {});
}
