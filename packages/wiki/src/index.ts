import type { Component } from 'svelte';

export interface WikiArticle {
	slug: string;
	title: string;
	description: string;
	order: number;
	component: Component;
}

interface SvxModule {
	default: Component;
	metadata?: { title?: string; description?: string; order?: number };
}

const modules = import.meta.glob<SvxModule>('./articles/*.svx', { eager: true });

export const articles: WikiArticle[] = Object.entries(modules)
	.map(([path, mod]) => {
		const slug = path.slice(path.lastIndexOf('/') + 1, -'.svx'.length);
		return {
			slug,
			title: mod.metadata?.title ?? slug,
			description: mod.metadata?.description ?? '',
			order: mod.metadata?.order ?? 0,
			component: mod.default
		};
	})
	.sort((a, b) => a.order - b.order || a.title.localeCompare(b.title));

export function getArticle(slug: string): WikiArticle | undefined {
	return articles.find((a) => a.slug === slug);
}

export { default as WikiVideo } from './wiki-video.svelte';
