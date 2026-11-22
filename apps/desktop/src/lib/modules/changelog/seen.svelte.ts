import { BROWSER as browser } from 'esm-env';
import { fetchChangelog } from './methods';

const SEEN_KEY = 'changelog:seenVersion';

function read(): string | null {
	if (!browser) return null;
	try {
		return window.localStorage.getItem(SEEN_KEY);
	} catch {
		return null;
	}
}

function write(version: string): void {
	if (!browser) return;
	try {
		window.localStorage.setItem(SEEN_KEY, version);
	} catch {}
}

/** Lights the "new release" dot until the changelog is opened. */
class ChangelogSeen {
	latest = $state<string | null>(null);
	seen = $state<string | null>(read());

	unread = $derived(this.latest !== null && this.seen !== null && this.latest !== this.seen);

	/** The newest published release; a first run counts it as seen, so a fresh install starts without a dot. */
	setLatest(version: string): void {
		this.latest = version;
		if (this.seen === null) this.markSeen();
	}

	markSeen(): void {
		if (this.latest === null) return;
		this.seen = this.latest;
		write(this.latest);
	}
}

export const changelogSeen = new ChangelogSeen();

let checked = false;
/** One look at GitHub per launch; failures leave the dot off rather than guessing. */
export function initChangelogWatch(): void {
	if (checked) return;
	checked = true;
	fetchChangelog()
		.then((entries) => {
			if (entries[0]) changelogSeen.setLatest(entries[0].version);
		})
		.catch(() => {});
}
