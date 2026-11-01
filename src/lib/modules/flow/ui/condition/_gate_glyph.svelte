<script lang="ts">
	import { usesB, type LogicOp } from '../../logic';

	interface Props {
		op: LogicOp;
		a: boolean;
		b: boolean;
		out: boolean;
		/** Output the audio takes: 1 true, 2 false. */
		route: number;
		/** Null while the inputs choose the route. */
		onRoute: ((branch: number) => void) | null;
	}
	let { op, a, b, out, route, onRoute }: Props = $props();

	// ANSI gate outlines in a 160 x 64 box. Inputs enter at a quarter and three
	// quarters of the height so they meet the sockets beside the glyph; the
	// output forks into the two routes the audio can take.
	const AND = 'M30 4 H56 A28 28 0 0 1 56 60 H30 Z';
	const OR = 'M26 4 Q60 4 84 32 Q60 60 26 60 Q42 32 26 4 Z';
	const BUFFER = 'M32 6 L80 32 L32 58 Z';
	const FORK = 104;
	const BRANCHES = [
		{ branch: 1, label: 'true', y: 12, d: `M${FORK} 32 C${FORK + 12} 32 ${FORK + 10} 12 ${FORK + 24} 12 H160` },
		{ branch: 2, label: 'false', y: 52, d: `M${FORK} 32 C${FORK + 12} 32 ${FORK + 10} 52 ${FORK + 24} 52 H160` }
	];

	let family = $derived(op === 'a' || op === 'not' ? 'buffer' : op === 'and' || op === 'nand' ? 'and' : 'or');
	let shape = $derived(family === 'buffer' ? BUFFER : family === 'and' ? AND : OR);
	let tip = $derived(family === 'buffer' ? 80 : 84);
	let inverted = $derived(op === 'not' || op === 'nand' || op === 'nor' || op === 'xnor');
	let exclusive = $derived(op === 'xor' || op === 'xnor');
	let two = $derived(usesB(op));
	let outStart = $derived(inverted ? tip + 8 : tip);

	function pick(e: KeyboardEvent, branch: number) {
		if (e.key !== 'Enter' && e.key !== ' ') return;
		e.preventDefault();
		onRoute?.(branch);
	}
</script>

{#snippet branch(br: (typeof BRANCHES)[number])}
	<path d={br.d} class={['gate-wire', route === br.branch && 'lit']} fill="none" />
	<text x="160" y={br.y + (br.branch === 1 ? -4 : 11)} text-anchor="end" class={['branch-label', route === br.branch && 'on']}>{br.label}</text>
	<path d={br.d} class="hit" fill="none" />
{/snippet}

<svg viewBox="0 0 160 64" class="gate h-14 w-36 overflow-visible" aria-label="Logic gate and route">
	{#if two}
		<line x1="0" y1="16" x2="40" y2="16" class={['gate-wire', a && 'lit']} />
		<line x1="0" y1="48" x2="40" y2="48" class={['gate-wire', b && 'lit']} />
	{:else}
		<line x1="0" y1="32" x2="40" y2="32" class={['gate-wire', a && 'lit']} />
	{/if}
	{#if exclusive}
		<path d="M18 4 Q34 32 18 60" class="gate-line" fill="none" />
	{/if}
	<path d={shape} class={['gate-shape', out && 'hot']} />
	{#if inverted}
		<circle cx={tip + 4} cy="32" r="4" class={['gate-shape', out && 'hot']} />
	{/if}
	<line x1={outStart} y1="32" x2={FORK} y2="32" class={['gate-wire', out && 'lit']} />

	{#each BRANCHES as br (br.branch)}
		{#if onRoute}
			<g
				class="branch nodrag nopan pickable"
				role="button"
				tabindex="0"
				aria-label={`Route to ${br.label}`}
				onclick={() => onRoute?.(br.branch)}
				onkeydown={(e) => pick(e, br.branch)}>
				{@render branch(br)}
			</g>
		{:else}
			<g class="branch">{@render branch(br)}</g>
		{/if}
	{/each}
	<circle cx={FORK} cy="32" r="2.5" class={['joint', out && 'lit']} />
</svg>

<style>
	.gate {
		--idle: var(--color-neutral-600);
		--glow: var(--color-signal);
	}
	.gate-wire,
	.gate-line {
		stroke: var(--idle);
		stroke-width: 2;
		stroke-linecap: round;
		transition: stroke 150ms;
	}
	.gate-wire.lit {
		stroke: var(--glow);
		filter: drop-shadow(0 0 3px var(--glow));
	}
	.gate-shape {
		fill: var(--color-neutral-100);
		stroke: var(--idle);
		stroke-width: 2;
		stroke-linejoin: round;
		transition:
			fill 150ms,
			stroke 150ms;
	}
	.gate-shape.hot {
		fill: color-mix(in oklch, var(--glow) 18%, var(--color-neutral-100));
		stroke: var(--glow);
	}
	.joint {
		fill: var(--idle);
	}
	.joint.lit {
		fill: var(--glow);
	}
	.branch-label {
		font:
			9px ui-monospace,
			monospace;
		fill: var(--color-neutral-900);
		transition: fill 150ms;
	}
	.branch-label.on {
		fill: var(--glow);
	}
	.hit {
		stroke: transparent;
		stroke-width: 14;
	}
	.pickable {
		cursor: pointer;
	}
	.pickable:hover .gate-wire:not(.lit) {
		stroke: var(--color-neutral-900);
	}
	.pickable:focus-visible {
		outline: none;
	}
	.pickable:focus-visible .branch-label {
		text-decoration: underline;
	}
</style>
