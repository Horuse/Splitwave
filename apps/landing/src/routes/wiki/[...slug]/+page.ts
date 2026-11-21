import { error } from '@sveltejs/kit';
import { articles, getNode } from '@splitwave/wiki';
import { wikiUrl } from '#lib/site.js';
import type { EntryGenerator } from './$types';

/** Every article is a page; folders without an index article are listed on /wiki only. */
export const entries: EntryGenerator = () => articles.filter((a) => a.slug).map((a) => ({ slug: a.slug }));

export const load = ({ params }) => {
	const node = getNode(params.slug);
	if (!node?.article) error(404, 'Article not found');
	return {
		slug: node.slug,
		seo: {
			title: `${node.article.title} — Splitwave Wiki`,
			description: node.article.description || `${node.article.title} in Splitwave, the free node-based audio router.`,
			url: wikiUrl(node.slug)
		}
	};
};
