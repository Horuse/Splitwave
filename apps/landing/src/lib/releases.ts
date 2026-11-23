const API = 'https://api.github.com/repos/Horuse/Splitwave';

// Unauthenticated GitHub allows 60 requests an hour per IP, and every visitor of
// the worker shares one. Caching is what keeps us under it.
const EDGE_TTL = 300;

const INIT = {
	headers: {
		'User-Agent': 'splitwave-landing',
		Accept: 'application/vnd.github+json'
	},
	// Cloudflare caches fetch() subrequests at the edge, shared across isolates.
	// A module-level memo would not survive, since each request may start cold.
	cf: { cacheTtl: EDGE_TTL, cacheEverything: true }
} as RequestInit;

export interface Asset {
	name: string;
	url: string;
	size: number;
	/** Hex SHA-256 from GitHub's asset digest; absent on assets uploaded before GitHub computed it. */
	sha256?: string;
}

export interface Release {
	tag: string;
	version: string;
	url: string;
	publishedAt: string;
	body: string;
	prerelease: boolean;
	assets: Asset[];
}

interface GhAsset {
	name: string;
	size: number;
	browser_download_url: string;
	digest?: string | null;
}

interface GhRelease {
	tag_name: string;
	html_url: string;
	published_at: string;
	body: string | null;
	draft: boolean;
	prerelease: boolean;
	assets: GhAsset[];
}

// Signatures and the updater manifest are machine files, never offered as downloads.
const HIDDEN = /(\.sig|latest\.json)$/;

function shape(r: GhRelease): Release {
	return {
		tag: r.tag_name,
		version: r.tag_name.replace(/^v/, ''),
		url: r.html_url,
		publishedAt: r.published_at,
		body: r.body ?? '',
		prerelease: r.prerelease,
		assets: r.assets
			.filter((a) => !HIDDEN.test(a.name))
			.map((a) => ({
				name: a.name,
				url: a.browser_download_url,
				size: a.size,
				sha256: a.digest?.startsWith('sha256:') ? a.digest.slice(7) : undefined
			}))
	};
}

// SvelteKit's event.fetch rebuilds the Request and can drop `cf`, so the
// platform fetch is used directly.
async function gh<T>(path: string): Promise<T | null> {
	const res = await fetch(`${API}${path}`, INIT);
	return res.ok ? ((await res.json()) as T) : null;
}

/** Null means GitHub was unreachable; callers surface that rather than guessing a version. */
export async function latestRelease(): Promise<Release | null> {
	const raw = await gh<GhRelease>('/releases/latest');
	return raw ? shape(raw) : null;
}

export async function allReleases(): Promise<Release[] | null> {
	const raw = await gh<GhRelease[]>('/releases?per_page=30');
	return raw ? raw.filter((r) => !r.draft).map(shape) : null;
}

/** Matches on the filename suffix so a version bump cannot break the lookup. */
export function pickAsset(assets: Asset[], suffix: string): Asset | undefined {
	return assets.find((a) => a.name.endsWith(suffix));
}

export function formatSize(bytes: number): string {
	return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

export function formatDate(iso: string): string {
	return new Date(iso).toLocaleDateString('en-US', {
		year: 'numeric',
		month: 'long',
		day: 'numeric'
	});
}
