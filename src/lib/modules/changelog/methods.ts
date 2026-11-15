const RELEASES_API = 'https://api.github.com/repos/Horuse/Splitwave/releases?per_page=30';

export interface ChangelogEntry {
	version: string;
	name: string;
	publishedAt: string;
	prerelease: boolean;
	notes: string;
	url: string;
}

interface GithubRelease {
	tag_name: string;
	name: string | null;
	published_at: string | null;
	prerelease: boolean;
	draft: boolean;
	body: string | null;
	html_url: string;
}

export async function fetchChangelog(): Promise<ChangelogEntry[]> {
	const res = await fetch(RELEASES_API, { headers: { Accept: 'application/vnd.github+json' } });
	if (!res.ok) throw new Error(`GitHub responded with ${res.status}`);
	const releases = (await res.json()) as GithubRelease[];
	return releases
		.filter((r) => !r.draft)
		.map((r) => ({
			version: r.tag_name.replace(/^v/, ''),
			name: r.name?.trim() || r.tag_name,
			publishedAt: r.published_at ?? '',
			prerelease: r.prerelease,
			notes: r.body?.trim() ?? '',
			url: r.html_url
		}));
}
