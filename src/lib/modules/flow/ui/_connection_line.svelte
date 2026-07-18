<script lang="ts">
	import { getBezierPath, useConnection } from '@xyflow/svelte';
	import { channelColor, parseHandle } from '$lib/modules/flow/utils';
	import { channelSelection } from '$lib/modules/flow/stores.svelte';

	const connection = useConnection();

	// A custom line replaces xyflow's default path, so the dragged one is drawn here too.
	let lines = $derived.by(() => {
		const c = connection.current;
		if (!c.inProgress) return [];

		const dragged = {
			ch: c.fromHandle?.id ? parseHandle(c.fromHandle.id) : null,
			x: c.from.x,
			y: c.from.y
		};

		const points = [dragged];
		const armed = channelSelection.channels;
		if (channelSelection.nodeId === c.fromHandle?.nodeId && armed.length > 1) {
			const bounds = c.fromNode.internals.handleBounds?.source ?? [];
			const origin = c.fromNode.internals.positionAbsolute;
			for (const ch of armed) {
				if (ch === dragged.ch) continue;
				const h = bounds.find((b) => b.id === `ch${ch}`);
				if (!h) continue;
				points.push({
					ch,
					x: origin.x + h.x + h.width / 2,
					y: origin.y + h.y + h.height / 2
				});
			}
		}

		return points.map((p) => ({
			ch: p.ch,
			color: p.ch === null ? '#a3a3a3' : channelColor(p.ch - 1),
			path: getBezierPath({
				sourceX: p.x,
				sourceY: p.y,
				sourcePosition: c.fromPosition,
				targetX: c.to.x,
				targetY: c.to.y,
				targetPosition: c.toPosition
			})[0]
		}));
	});
</script>

{#each lines as line (line.ch)}
	<path d={line.path} fill="none" stroke={line.color} stroke-width="2" />
{/each}
