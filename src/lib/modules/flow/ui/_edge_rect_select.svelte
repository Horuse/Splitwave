<script lang="ts">
	import { untrack } from 'svelte';
	import { useStore } from '@xyflow/svelte';

	// xyflow's rect only selects edges running between two selected nodes.
	// Owns the selection outright: xyflow skips its write when its own set is
	// unchanged, so additive hits would stay stuck once the rect moved off them.
	const store = useStore();

	const SAMPLES = 16;

	function pathOf(edgeId: string): SVGPathElement | null {
		return document.querySelector(`.svelte-flow__edge[data-id="${CSS.escape(edgeId)}"] .svelte-flow__edge-path`);
	}

	let lastKey = '';

	$effect(() => {
		const rect = store.selectionRect;
		const mode = store.selectionRectMode;
		const vp = store.viewport;

		if (!rect || mode !== 'user') {
			lastKey = '';
			return;
		}

		untrack(() => {
			const selectedNodes = new Set(store.nodes.filter((n) => n.selected).map((n) => n.id));
			const hit = new Set<string>();

			for (const edge of store.edges) {
				if (selectedNodes.has(edge.source) || selectedNodes.has(edge.target)) {
					hit.add(edge.id);
					continue;
				}
				const el = pathOf(edge.id);
				if (!el) continue;
				const len = el.getTotalLength();
				for (let i = 0; i <= SAMPLES; i++) {
					const p = el.getPointAtLength((len * i) / SAMPLES);
					const x = p.x * vp.zoom + vp.x;
					const y = p.y * vp.zoom + vp.y;
					if (x >= rect.x && x <= rect.x + rect.width && y >= rect.y && y <= rect.y + rect.height) {
						hit.add(edge.id);
						break;
					}
				}
			}

			const key = [...hit].sort().join(',');
			if (key === lastKey) return;
			lastKey = key;
			store.edges = store.edges.map((e) => (!!e.selected === hit.has(e.id) ? e : { ...e, selected: hit.has(e.id) }));
		});
	});
</script>
