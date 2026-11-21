import { articles } from '@splitwave/wiki';
import { SITE, wikiUrl } from '#lib/site.js';

export const prerender = true;

const PAGES: { url: string; changefreq: string; priority: string }[] = [
	{ url: `${SITE}/`, changefreq: 'monthly', priority: '1.0' },
	{ url: `${SITE}/download`, changefreq: 'monthly', priority: '0.9' },
	{ url: `${SITE}/wiki`, changefreq: 'weekly', priority: '0.8' },
	{ url: `${SITE}/changelog`, changefreq: 'weekly', priority: '0.7' }
];

/** Built from the wiki package, so a new article is listed without editing this file. */
export function GET() {
	const wiki = articles
		.filter((a) => a.slug)
		.map((a) => ({ url: wikiUrl(a.slug), changefreq: 'monthly', priority: '0.6' }));
	const body = [...PAGES, ...wiki]
		.map((p) => `\t<url>\n\t\t<loc>${p.url}</loc>\n\t\t<changefreq>${p.changefreq}</changefreq>\n\t\t<priority>${p.priority}</priority>\n\t</url>`)
		.join('\n');
	return new Response(`<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n${body}\n</urlset>\n`, {
		headers: { 'content-type': 'application/xml; charset=utf-8' }
	});
}
