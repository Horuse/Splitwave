/**
 * Lifts a node's canvas out of the xyflow viewport into the flow root, on top
 * of its place in the node.
 *
 * WebKit (Safari, Tauri on macOS) makes a canvas a composited layer. Inside the
 * viewport, which zooms with a CSS transform, that turns the whole viewport into
 * one bitmap drawn at scale 1 and stretched, so every node on the canvas blurs.
 * Outside the viewport the canvas is sized in screen pixels instead, so the
 * nodes stay sharp and the canvas itself is drawn at the real zoom.
 */
export interface FloatingCanvas {
	/** Screen pixels per node pixel: the flow zoom. Multiply devicePixelRatio by it. */
	readonly scale: number;
	destroy(): void;
}

interface Options {
	/** Runs when the zoom changes, so the canvas redraws at the new resolution. */
	onScale: () => void;
	/** Elements in the node drawn over the canvas; the canvas leaves a hole of each one's shape. */
	holes?: () => Iterable<Element>;
	/** Corner radius in node pixels. */
	radius?: number;
}

export function floatCanvas(canvas: HTMLCanvasElement, anchor: HTMLElement, { onScale, holes, radius = 0 }: Options): FloatingCanvas {
	const host = anchor.closest<HTMLElement>('.svelte-flow');
	// A preview outside a flow is not zoomed: the canvas stays in the node.
	if (!host) return { scale: 1, destroy() {} };

	host.appendChild(canvas);
	Object.assign(canvas.style, { position: 'absolute', left: '0', top: '0', pointerEvents: 'none', zIndex: '4' });

	let scale = 1;
	let raf = 0;
	let last = '';

	function sync() {
		raf = requestAnimationFrame(sync);
		const width = anchor.offsetWidth;
		if (!anchor.isConnected || width === 0) {
			if (canvas.style.display !== 'none') canvas.style.display = 'none';
			return;
		}
		const r = anchor.getBoundingClientRect();
		const h = host!.getBoundingClientRect();
		const left = r.left - h.left;
		const top = r.top - h.top;
		const next = r.width / width;

		let clip = '';
		for (const el of holes?.() ?? []) {
			const b = el.getBoundingClientRect();
			if (b.width === 0) continue;
			const x = b.left - r.left;
			const y = b.top - r.top;
			const w = b.width;
			const hh = b.height;
			// Same corners as the element, so its rounded shape shows through, not a box.
			const k = Math.min(parseFloat(getComputedStyle(el).borderTopLeftRadius) * next || 0, w / 2, hh / 2);
			clip += ` M${x + k} ${y} H${x + w - k} A${k} ${k} 0 0 1 ${x + w} ${y + k} V${y + hh - k} A${k} ${k} 0 0 1 ${x + w - k} ${y + hh} H${x + k} A${k} ${k} 0 0 1 ${x} ${y + hh - k} V${y + k} A${k} ${k} 0 0 1 ${x + k} ${y} Z`;
		}
		const key = `${left} ${top} ${r.width} ${r.height}${clip}`;
		if (key !== last) {
			last = key;
			canvas.style.display = 'block';
			canvas.style.transform = `translate(${left}px, ${top}px)`;
			canvas.style.width = `${r.width}px`;
			canvas.style.height = `${r.height}px`;
			canvas.style.borderRadius = `${radius * next}px`;
			canvas.style.clipPath = clip ? `path(evenodd, 'M0 0 H${r.width} V${r.height} H0 Z${clip}')` : '';
		}
		if (next !== scale) {
			scale = next;
			onScale();
		}
	}
	raf = requestAnimationFrame(sync);

	return {
		get scale() {
			return scale;
		},
		destroy() {
			cancelAnimationFrame(raf);
			canvas.remove();
		}
	};
}
