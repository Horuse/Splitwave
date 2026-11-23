import type { PageServerLoad } from './$types';
import { latestRelease } from '#lib/releases.js';

export const load: PageServerLoad = async ({ setHeaders }) => {
	const release = await latestRelease();

	setHeaders({ 'cache-control': 'public, max-age=300' });

	return {
		seo: {
			title: 'Splitwave — Audio Routing App for macOS, Linux and Windows',
			url: 'https://splitwave.app'
		},
		release
	};
};
