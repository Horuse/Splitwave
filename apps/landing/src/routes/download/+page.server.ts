import { latestRelease } from '#lib/releases.js';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ setHeaders }) => {
	const release = await latestRelease();

	setHeaders({ 'cache-control': 'public, max-age=300' });

	const version = release ? ` ${release.version}` : '';

	return {
		release,
		seo: {
			title: 'Download Splitwave for macOS, Linux and Windows',
			description: `Download Splitwave${version} for macOS, Linux and Windows — with step-by-step installation instructions.`,
			url: 'https://splitwave.app/download'
		}
	};
};
