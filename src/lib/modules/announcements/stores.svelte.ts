import { browser } from '$app/environment';
import type { Announcement } from './types';

const DISMISSED_KEY = 'announcements:dismissed';
const SEEN_KEY = 'announcements:seen';

function loadSeen(): Set<string> {
	if (!browser) return new Set();
	try {
		const parsed: unknown = JSON.parse(window.localStorage.getItem(SEEN_KEY) ?? '[]');
		return new Set(Array.isArray(parsed) ? parsed.filter((v): v is string => typeof v === 'string') : []);
	} catch {
		return new Set();
	}
}

function saveSeen(ids: Set<string>): void {
	if (!browser) return;
	try {
		window.localStorage.setItem(SEEN_KEY, JSON.stringify([...ids]));
	} catch {}
}

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
	/** Every announcement for this install, dismissed ones included: the list page shows them all. */
	all = $state<Announcement[]>([]);
	/** Ids the user has opened or listed; anything else lights the "new" dot. */
	seen = $state<Set<string>>(loadSeen());

	unreadCount = $derived(this.all.filter((a) => !this.seen.has(a.id)).length);

	isUnread(id: string): boolean {
		return !this.seen.has(id);
	}

	markSeen(ids: string[]): void {
		const missing = ids.filter((id) => !this.seen.has(id));
		if (missing.length === 0) return;
		const next = new Set(this.seen);
		for (const id of missing) next.add(id);
		this.seen = next;
		saveSeen(next);
	}

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
