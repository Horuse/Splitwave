import { browser } from '$app/environment';
import type { Announcement } from './types';

const DISMISSED_KEY = 'announcements:dismissed';

function loadDismissed(): Record<string, number> {
	if (!browser) return {};
	try {
		const raw = window.localStorage.getItem(DISMISSED_KEY);
		if (!raw) return {};
		const parsed: unknown = JSON.parse(raw);
		if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) return {};
		return Object.fromEntries(
			Object.entries(parsed).filter((entry): entry is [string, number] => typeof entry[1] === 'number' && Number.isFinite(entry[1]))
		);
	} catch {
		return {};
	}
}

function saveDismissed(map: Record<string, number>): void {
	if (!browser) return;
	try {
		window.localStorage.setItem(DISMISSED_KEY, JSON.stringify(map));
	} catch {}
}

class AnnouncementStore {
	bannerQueue = $state<Announcement[]>([]);
	currentBannerIndex = $state(0);
	modalQueue = $state<Announcement[]>([]);
	isModalActive = $state(false);
	dismissed = $state<Record<string, number>>(loadDismissed());
	isLoading = $state(false);

	activeBanner = $derived.by(() => {
		if (this.bannerQueue.length === 0) return null;
		if (this.currentBannerIndex >= this.bannerQueue.length) {
			return this.bannerQueue[0];
		}
		return this.bannerQueue[this.currentBannerIndex];
	});

	bannerCount = $derived(this.bannerQueue.length);

	dismiss(id: string): void {
		const now = Date.now();
		const next = { ...this.dismissed, [id]: now };
		this.dismissed = next;
		saveDismissed(next);

		const bannerIdx = this.bannerQueue.findIndex((b) => b.id === id);
		if (bannerIdx !== -1) {
			this.bannerQueue = this.bannerQueue.filter((b) => b.id !== id);
			if (this.currentBannerIndex >= this.bannerQueue.length) {
				this.currentBannerIndex = Math.max(0, this.bannerQueue.length - 1);
			}
		}

		this.modalQueue = this.modalQueue.filter((m) => m.id !== id);
	}

	nextBanner(): void {
		if (this.bannerQueue.length <= 1) return;
		this.currentBannerIndex = (this.currentBannerIndex + 1) % this.bannerQueue.length;
	}

	prevBanner(): void {
		if (this.bannerQueue.length <= 1) return;
		this.currentBannerIndex = (this.currentBannerIndex - 1 + this.bannerQueue.length) % this.bannerQueue.length;
	}
}

export const announcementStore = new AnnouncementStore();
