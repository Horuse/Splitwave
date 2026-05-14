<script lang="ts">
	import type { Snippet } from 'svelte';
	import { Handle, Position } from '@xyflow/svelte';

	export interface InputHandleConfig {
		id: string;
		label?: string;
		position?: 'left' | 'bottom' | 'top';
	}

	interface Props {
		label: string;
		accent?: 'input' | 'output' | 'effect';
		hasInput?: boolean;
		hasOutput?: boolean;
		inputs?: InputHandleConfig[];
		outputLabel?: string;
		bypassed?: boolean;
		onBypass?: () => void;
		children?: Snippet;
	}

	let {
		label,
		accent = 'effect',
		hasInput = false,
		hasOutput = false,
		inputs,
		outputLabel,
		bypassed,
		onBypass,
		children
	}: Props = $props();

	function pos(p: InputHandleConfig['position']): Position {
		if (p === 'bottom') return Position.Bottom;
		if (p === 'top') return Position.Top;
		return Position.Left;
	}

	function labelClasses(p: InputHandleConfig['position']): string {
		const base = 'pointer-events-none absolute px-1 font-mono text-[9px] leading-none text-neutral-700';
		if (p === 'bottom') return `${base} whitespace-nowrap bottom-full mb-0.5 left-1/2 -translate-x-1/2`;
		if (p === 'top') return `${base} whitespace-nowrap top-full mt-0.5 left-1/2 -translate-x-1/2`;
		return `${base} left-full ml-0.5 top-1/2 -translate-y-1/2 [writing-mode:vertical-lr]`;
	}
</script>

<div
	class={[
		'min-w-32 max-w-80 rounded-2xl border border-neutral-400 bg-neutral-200 p-4 shadow-sm',
	]}
>
	<div class="mb-2 flex items-center justify-between gap-2">
		<span class="text-[10px] font-semibold tracking-wider text-neutral-900 uppercase">
			{label}
		</span>
		{#if onBypass}
			<button
				type="button"
				class={[
					'nodrag nopan flex h-4 items-center rounded border px-1.5 font-mono text-[9px] transition-colors',
					bypassed
						? 'border-amber-500 bg-amber-100 text-amber-900 hover:bg-amber-200'
						: 'border-neutral-400 bg-neutral-100 text-neutral-900 hover:bg-neutral-200'
				]}
				title={bypassed ? 'Bypassed -- click to engage' : 'Engaged -- click to bypass'}
				onclick={onBypass}
			>
				{bypassed ? 'BYP' : 'ON'}
			</button>
		{/if}
	</div>

	<div class={bypassed ? 'opacity-40' : ''}>
		{@render children?.()}
	</div>

	{#if inputs && inputs.length > 0}
		{#each inputs as h (h.id)}
			<Handle type="target" id={h.id} class="handle" position={pos(h.position)}>
				{#if h.label}
					<span class={labelClasses(h.position)}>{h.label}</span>
				{/if}
			</Handle>
		{/each}
	{:else if hasInput}
		<Handle type="target" class="handle" position={Position.Left} />
	{/if}
	{#if hasOutput}
		<Handle type="source" class="handle" position={Position.Right}>
			{#if outputLabel}
				<span class="pointer-events-none absolute right-full mr-0.5 top-1/2 -translate-y-1/2 px-1 font-mono text-[9px] leading-none text-neutral-700 [writing-mode:vertical-rl]">
					{outputLabel}
				</span>
			{/if}
		</Handle>
	{/if}
</div>
