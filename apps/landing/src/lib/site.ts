export const SITE = 'https://splitwave.app';

/** Canonical URL of a wiki page; the index lives at /wiki. */
export function wikiUrl(slug: string): string {
	return slug ? `${SITE}/wiki/${slug}` : `${SITE}/wiki`;
}
