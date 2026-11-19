import { allReleases, latestRelease } from '#lib/releases.js';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ setHeaders }) => {
	const [releases, latest] = await Promise.all([allReleases(), latestRelease()]);

	setHeaders({ 'cache-control': 'public, max-age=1800' });

	return {
		releases,
		// GitHub's /releases/latest skips pre-releases, so it is the source of truth for the badge.
		latestTag: latest?.tag ?? releases?.find((r) => !r.prerelease)?.tag ?? null,
		seo: {
			title: 'Splitwave changelog — every release',
			description:
				'Release notes for Splitwave, the free open-source audio routing app for macOS, Linux and Windows. New nodes, effects, and fixes in every version.',
			url: 'https://splitwave.app/changelog'
		}
	};
};
