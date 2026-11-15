import type { Attachment } from 'svelte/attachments';

interface RadiusOptions {
	base?: number;
	ref?: number;
	exponent?: number;
	min?: number;
	max?: number;
	variable?: string;
	measure?: (node: HTMLElement) => HTMLElement | null;
	side?: number;
	maxRatio?: number;
	nested?: boolean;
}

// Radius grows with the element size: base * (min(w, h) / ref) ^ exponent, capped at maxRatio of the short side.
type Apply = (width: number, height: number) => void;
const targets = new WeakMap<Element, Apply>();
let shared: ResizeObserver | undefined;

function observe(node: HTMLElement, apply: Apply): () => void {
	shared ??= new ResizeObserver((entries) => {
		for (const entry of entries) {
			const box = entry.borderBoxSize?.[0];
			if (box) targets.get(entry.target)?.(box.inlineSize, box.blockSize);
		}
	});
	targets.set(node, apply);
	shared.observe(node);
	return () => {
		shared?.unobserve(node);
		targets.delete(node);
	};
}

export function smoothRadius({
	base = 12,
	ref = 36,
	exponent = 0.25,
	min = 6,
	max = 32,
	variable,
	measure,
	side: fixedSide,
	maxRatio = 0.5,
	nested = false
}: RadiusOptions = {}): Attachment<HTMLElement> {
	return (node) => {
		let side = 0;
		const render = () => {
			if (!side) return;
			const inner = Math.min(side * maxRatio, max, Math.max(min, base * Math.pow(side / ref, exponent)));
			const radius = nested ? inner + (parseFloat(getComputedStyle(node).paddingLeft) || 0) : inner;
			const value = `${radius.toFixed(1)}px`;
			if (variable) node.style.setProperty(variable, value);
			else node.style.borderRadius = value;
		};
		const apply: Apply = (width, height) => {
			side = Math.min(width, height);
			render();
		};
		if (fixedSide) {
			apply(fixedSide, fixedSide);
			return () => {
				if (variable) node.style.removeProperty(variable);
				else node.style.borderRadius = '';
			};
		}
		const source = (nested ? (node.firstElementChild as HTMLElement | null) : measure?.(node)) ?? node;
		apply(source.offsetWidth, source.offsetHeight);
		const stop = observe(source, apply);
		return () => {
			stop();
			if (variable) node.style.removeProperty(variable);
			else node.style.borderRadius = '';
		};
	};
}

const SMOOTH_SELECTOR = '.smooth';

export function watchSmoothRadius(root: HTMLElement = document.documentElement): () => void {
	const active = new Map<Element, () => void>();
	const attach = (node: Element) => {
		if (active.has(node) || !(node instanceof HTMLElement)) return;
		const { radiusBase, radiusSide, radiusNested } = node.dataset;
		const cleanup = smoothRadius({
			variable: '--smooth-r',
			...(radiusBase && { base: Number(radiusBase) }),
			...(radiusSide && { side: Number(radiusSide) }),
			...(radiusNested !== undefined && { nested: true })
		})(node);
		active.set(node, typeof cleanup === 'function' ? cleanup : () => {});
	};
	const detach = (node: Element) => {
		active.get(node)?.();
		active.delete(node);
	};
	const scan = (node: Node, apply: (el: Element) => void) => {
		if (!(node instanceof Element)) return;
		if (node.matches(SMOOTH_SELECTOR)) apply(node);
		node.querySelectorAll(SMOOTH_SELECTOR).forEach(apply);
	};

	scan(root, attach);
	const mutations = new MutationObserver((records) => {
		for (const record of records) {
			if (record.type === 'attributes') {
				const el = record.target as Element;
				if (el.matches(SMOOTH_SELECTOR)) attach(el);
				else detach(el);
				continue;
			}
			record.removedNodes.forEach((node) => scan(node, (el) => !el.isConnected && detach(el)));
			record.addedNodes.forEach((node) => scan(node, attach));
		}
	});
	mutations.observe(root, { subtree: true, childList: true, attributes: true, attributeFilter: ['class'] });

	return () => {
		mutations.disconnect();
		active.forEach((cleanup) => cleanup());
		active.clear();
	};
}
