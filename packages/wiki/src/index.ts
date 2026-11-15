import type { Component } from 'svelte';

export interface WikiArticle {
	slug: string;
	title: string;
	description: string;
	order: number;
	component: Component;
}

export interface WikiNode {
	slug: string;
	title: string;
	order: number;
	article?: WikiArticle;
	children: WikiNode[];
}

interface SvxModule {
	default: Component;
	metadata?: { title?: string; description?: string; order?: number };
}

const modules = import.meta.glob<SvxModule>('./articles/**/*.svx', { eager: true });

function titleFromSlug(slug: string): string {
	const last = slug.slice(slug.lastIndexOf('/') + 1).replace(/-/g, ' ');
	return last.charAt(0).toUpperCase() + last.slice(1);
}

// A folder's index.svx is the page of the folder itself.
export const articles: WikiArticle[] = Object.entries(modules).map(([path, mod]) => {
	const rel = path.slice('./articles/'.length, -'.svx'.length);
	const slug = rel === 'index' ? '' : rel.replace(/\/index$/, '');
	return {
		slug,
		title: mod.metadata?.title ?? titleFromSlug(slug),
		description: mod.metadata?.description ?? '',
		order: mod.metadata?.order ?? 0,
		component: mod.default
	};
});

function buildTree(): WikiNode[] {
	const root: WikiNode = { slug: '', title: '', order: 0, children: [] };
	const nodes = new Map<string, WikiNode>([['', root]]);
	const ensure = (slug: string): WikiNode => {
		const existing = nodes.get(slug);
		if (existing) return existing;
		const node: WikiNode = { slug, title: titleFromSlug(slug), order: 0, children: [] };
		nodes.set(slug, node);
		const parent = ensure(slug.includes('/') ? slug.slice(0, slug.lastIndexOf('/')) : '');
		parent.children.push(node);
		return node;
	};
	for (const article of articles) {
		if (!article.slug) continue;
		const node = ensure(article.slug);
		node.article = article;
		node.title = article.title;
		node.order = article.order;
	}
	const sort = (list: WikiNode[]) => {
		list.sort((a, b) => a.order - b.order || a.title.localeCompare(b.title));
		list.forEach((n) => sort(n.children));
	};
	sort(root.children);
	return root.children;
}

export const tree: WikiNode[] = buildTree();

export function getArticle(slug: string): WikiArticle | undefined {
	return articles.find((a) => a.slug === slug);
}

export function getNode(slug: string): WikiNode | undefined {
	const walk = (list: WikiNode[]): WikiNode | undefined => {
		for (const n of list) {
			if (n.slug === slug) return n;
			const found = walk(n.children);
			if (found) return found;
		}
	};
	return walk(tree);
}

export { default as WikiVideo } from './wiki-video.svelte';
