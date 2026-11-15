import { error } from '@sveltejs/kit';
import { getArticle } from '@splitwave/wiki';

export function load({ params }) {
	const article = getArticle(params.slug);
	if (!article) error(404, 'Article not found');
	return { slug: article.slug };
}
