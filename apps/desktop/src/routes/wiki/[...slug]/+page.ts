import { error } from '@sveltejs/kit';
import { getNode } from '@splitwave/wiki';

export function load({ params }) {
	const node = getNode(params.slug);
	if (!node) error(404, 'Article not found');
	return { slug: node.slug };
}
