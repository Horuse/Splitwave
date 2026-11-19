import type { RequestHandler } from './$types';

const EXCLUDED = ['latest.json'];
const EDGE_TTL = 3600; // 1 hour at Cloudflare Edge

// Last known fallback if cold start + GitHub rate limit hits simultaneously
const FALLBACK_DOWNLOADS = 880;

let memoryCache: { count: number; ts: number } | null = null;
const MEMORY_TTL = 10 * 60 * 1000; // 10 minutes in memory

export const GET: RequestHandler = async () => {
	if (!memoryCache || Date.now() - memoryCache.ts >= MEMORY_TTL) {
		try {
			// Using global fetch with cf cache parameters to leverage Cloudflare Edge cache
			const res = await fetch(
				'https://api.github.com/repos/Horuse/Splitwave/releases?per_page=100',
				{
					headers: {
						'User-Agent': 'splitwave-landing',
						Accept: 'application/vnd.github+json'
					},
					// Cloudflare caches subrequests across worker invocations and data centers
					cf: {
						cacheTtl: EDGE_TTL,
						cacheEverything: true
					}
				} as RequestInit
			);

			if (res.ok) {
				const releases: { assets?: { name: string; download_count: number }[] }[] =
					await res.json();
				let count = 0;
				for (const release of releases) {
					if (Array.isArray(release.assets)) {
						for (const asset of release.assets) {
							if (!EXCLUDED.includes(asset.name)) {
								count += asset.download_count;
							}
						}
					}
				}

				if (count > 0) {
					memoryCache = { count, ts: Date.now() };
				}
			}
		} catch {
			// Ignore network/fetch errors, use cached or fallback count
		}
	}

	const downloads = memoryCache?.count ?? FALLBACK_DOWNLOADS;

	return Response.json(
		{ downloads },
		{
			headers: {
				'Access-Control-Allow-Origin': '*',
				// Cache at edge and browsers, revalidate in background
				'Cache-Control': 'public, max-age=300, s-maxage=1800, stale-while-revalidate=86400'
			}
		}
	);
};
