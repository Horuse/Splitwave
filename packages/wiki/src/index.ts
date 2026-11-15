import type { Component } from 'svelte';

export interface WikiArticle {
	slug: string;
	title: string;
	description: string;
	order: number;
	/** File path inside src/articles, used for the GitHub edit link. */
	file: string;
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
		file: path.slice('./articles/'.length),
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

const REPO_EDIT = 'https://github.com/Horuse/Splitwave/edit/main/packages/wiki/src/articles/';

export function editUrl(article: WikiArticle): string {
	return REPO_EDIT + article.file;
}

/** Pages in reading order: depth-first through the tree, folders before their children. */
export const ordered: WikiNode[] = (() => {
	const out: WikiNode[] = [];
	const walk = (list: WikiNode[]) => {
		for (const n of list) {
			out.push(n);
			walk(n.children);
		}
	};
	walk(tree);
	return out;
})();

export function neighbours(slug: string): { prev?: WikiNode; next?: WikiNode } {
	const i = ordered.findIndex((n) => n.slug === slug);
	return i < 0 ? {} : { prev: ordered[i - 1], next: ordered[i + 1] };
}

export { default as WikiVideo } from './wiki-video.svelte';
