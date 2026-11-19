<script lang="ts">
	import { usesB, type LogicOp } from '@splitwave/nodes';

	interface Props {
		op: LogicOp;
		a: boolean;
		b: boolean;
		out: boolean;
		/** Output the audio takes: 1 true, 2 false. Absent on a signal-only gate,
		 * whose output runs straight out instead of forking. */
		route?: number;
		/** Null while the inputs choose the route. */
		onRoute?: ((branch: number) => void) | null;
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

	const WIRE = 'fill-none stroke-2 [stroke-linecap:round]';
	const SHAPE = 'stroke-2 [stroke-linejoin:round]';

	function wire(lit: boolean | undefined): string {
		return `${WIRE} ${lit ? 'stroke-signal' : 'stroke-neutral-600 group-hover/route:stroke-neutral-900'}`;
	}

	function shapeClass(lit: boolean): string {
		return `${SHAPE} ${lit ? 'stroke-signal fill-[color-mix(in_srgb,var(--color-signal)_18%,var(--color-neutral-100))]' : 'stroke-neutral-600 fill-neutral-100'}`;
	}

	function pick(e: KeyboardEvent, branch: number) {
		if (e.key !== 'Enter' && e.key !== ' ') return;
		e.preventDefault();
		onRoute?.(branch);
	}
</script>

{#snippet branch(br: (typeof BRANCHES)[number])}
	<path d={br.d} class={wire(route === br.branch)} />
	<text
		x="160"
		y={br.y + (br.branch === 1 ? -4 : 11)}
		text-anchor="end"
		class={['font-mono text-[9px] group-focus-visible/route:underline', route === br.branch ? 'fill-signal' : 'fill-neutral-900']}>{br.label}</text>
	<path d={br.d} class="fill-none stroke-transparent stroke-[14]" />
{/snippet}

<svg viewBox="0 0 160 64" class="h-14 w-36 overflow-visible" aria-label="Logic gate and route">
	{#if two}
		<line x1="0" y1="16" x2="40" y2="16" class={wire(a)} />
		<line x1="0" y1="48" x2="40" y2="48" class={wire(b)} />
	{:else}
		<line x1="0" y1="32" x2="40" y2="32" class={wire(a)} />
	{/if}
	{#if exclusive}
		<path d="M18 4 Q34 32 18 60" class={wire(false)} />
	{/if}
	<path d={shape} class={shapeClass(out)} />
	{#if inverted}
		<circle cx={tip + 4} cy="32" r="4" class={shapeClass(out)} />
	{/if}
	<line x1={outStart} y1="32" x2={route === undefined ? 160 : FORK} y2="32" class={wire(out)} />

	{#if route !== undefined}
		{#each BRANCHES as br (br.branch)}
			{#if onRoute}
				<g
					class="group/route nodrag nopan cursor-pointer outline-none"
					role="button"
					tabindex="0"
					aria-label={`Route to ${br.label}`}
					onclick={() => onRoute?.(br.branch)}
					onkeydown={(e) => pick(e, br.branch)}>
					{@render branch(br)}
				</g>
			{:else}
				<g>{@render branch(br)}</g>
			{/if}
		{/each}
		<circle cx={FORK} cy="32" r="2.5" class={out ? 'fill-signal' : 'fill-neutral-600'} />
	{/if}
</svg>
