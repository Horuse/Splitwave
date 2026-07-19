<script lang="ts">
	import { page } from '$app/state';
	import { platform } from '@tauri-apps/plugin-os';
	import Header from '$lib/components/layout/header.svelte';
	import { edgeSettings, type EdgeShape } from '$lib/modules/flow/edge_settings.svelte';
	import EdgeShapeIcon from '$lib/modules/flow/ui/_edge_shape_icon.svelte';
	import Toggle from '$lib/components/toggle.svelte';

	const isWindows = platform() === 'windows';

	const SHAPES: { value: EdgeShape; label: string; hint: string }[] = [
		{ value: 'bezier', label: 'Bezier', hint: 'Smooth curve, the default' },
		{ value: 'smoothstep', label: 'Smooth step', hint: 'Right angles, rounded' },
		{ value: 'step', label: 'Step', hint: 'Sharp right angles' },
		{ value: 'straight', label: 'Straight', hint: 'Direct line' }
	];

	function setShape(shape: EdgeShape) {
		edgeSettings.shape = shape;
		edgeSettings.persist();
	}

	function toggle(key: 'animated' | 'pins') {
		edgeSettings[key] = !edgeSettings[key];
		edgeSettings.persist();
	}
</script>

<Header>
	{#snippet left()}
		<div class="flex items-center gap-2">
			<a class:active={page.route.id === '/'} href="/" class="button-header px-4 text-sm">Pipelines</a>
			{#if !isWindows}
				<a
					class:active={page.route.id === '/virtual-devices'}
					href="/virtual-devices"
					class="button-header px-4 text-sm">Virtual devices</a
				>
			{/if}
			<a
				class:active={page.route.id === '/settings'}
				href="/settings"
				class="button-header px-4 text-sm">Settings</a
			>
		</div>
	{/snippet}
</Header>

<div class="h-[calc(100vh-40px)] overflow-y-auto p-8">
	<div class="flex max-w-2xl flex-col gap-8">
		<section class="flex flex-col gap-3">
			<div>
				<h2 class="text-sm font-semibold text-theme">Cable shape</h2>
				<p class="text-xs text-neutral-900">How connections are routed between nodes.</p>
			</div>

			<div class="grid grid-cols-2 gap-2 sm:grid-cols-4">
				{#each SHAPES as s (s.value)}
					<button
						type="button"
						onclick={() => setShape(s.value)}
						title={s.hint}
						class={[
							'flex flex-col items-center gap-2 rounded-xl border p-3 transition-colors',
							edgeSettings.shape === s.value
								? 'border-neutral-900 bg-neutral-200 text-theme'
								: 'border-neutral-400 bg-neutral-100 text-neutral-1000 hover:bg-neutral-200'
						]}
					>
						<EdgeShapeIcon shape={s.value} animated={edgeSettings.animated} />
						<span class="text-[11px] font-medium">{s.label}</span>
					</button>
				{/each}
			</div>
		</section>

		<section class="flex flex-col gap-2">
			<div>
				<h2 class="text-sm font-semibold text-theme">Rendering</h2>
				<p class="text-xs text-neutral-900">
					Turn these off first if the canvas drops frames on a large graph.
				</p>
			</div>

			{#each [{ key: 'animated' as const, label: 'Animate cables', hint: 'Marching dashes along every connection. Runs a CSS animation per edge.' }, { key: 'pins' as const, label: 'Connector pins', hint: 'Draws a plug at each end of a cable.' }] as row (row.key)}
				<Toggle
					checked={edgeSettings[row.key]}
					label={row.label}
					hint={row.hint}
					onChange={() => toggle(row.key)}
				/>
			{/each}
		</section>

		<button
			type="button"
			class="button-main primary self-start text-xs"
			onclick={() => edgeSettings.reset()}
		>
			Reset to defaults
		</button>
	</div>
</div>
