declare module '*.svx' {
	import type { Component } from 'svelte';
	const component: Component;
	export default component;
	export const metadata: Record<string, unknown>;
}
