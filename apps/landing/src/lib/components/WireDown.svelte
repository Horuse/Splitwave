<script lang="ts">
	interface Props {
		color: string;
		/** Draws a second lane flowing back up, for two-way links. */
		both?: boolean;
	}
	let { color, both = false }: Props = $props();

	// Markers live in defs and are referenced by id, so each instance needs its own.
	const uid = $props.id();
</script>

<svg
	width={both ? 24 : 14}
	height="44"
	viewBox={both ? '0 0 24 44' : '0 0 14 44'}
	class="shrink-0"
	aria-hidden="true"
>
	<defs>
		<marker
			id="down-{uid}"
			viewBox="0 0 8 8"
			refX="7"
			refY="4"
			markerWidth="5"
			markerHeight="5"
			orient="auto"
		>
			<path d="M0 0 L8 4 L0 8 z" fill={color} />
		</marker>
	</defs>
	<path class="wire" d="M{both ? 7 : 7} 2 V36" stroke={color} marker-end="url(#down-{uid})" />
	{#if both}
		<path
			class="wire"
			d="M17 42 V8"
			stroke={color}
			stroke-opacity="0.6"
			marker-end="url(#down-{uid})"
		/>
	{/if}
</svg>
