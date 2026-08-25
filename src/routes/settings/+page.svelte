<script lang="ts">
	import { page } from '$app/state';
	import { platform } from '@tauri-apps/plugin-os';
	import { enable as enableAutostart, disable as disableAutostart } from '@tauri-apps/plugin-autostart';
	import { onMount } from 'svelte';
	import Header from '$lib/components/layout/header.svelte';
	import { edgeSettings, type EdgeShape } from '$lib/modules/flow/edge_settings.svelte';
	import EdgeShapeIcon from '$lib/modules/flow/ui/_edge_shape_icon.svelte';
	import Toggle from '$lib/components/toggle.svelte';
	import { themeStore, type ThemePref } from '$lib/modules/theme/stores';
	import { appSettings, GRID_SIZES, SNAPSHOT_LIMITS } from '$lib/modules/settings/stores.svelte';
	import PresetsSection from './_presets_section.svelte';

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

	const THEMES: { value: ThemePref; label: string }[] = [
		{ value: 'light', label: 'Light' },
		{ value: 'dark', label: 'Dark' },
		{ value: 'system', label: 'System' }
	];

	function resetAll() {
		edgeSettings.reset();
		appSettings.reset();
		void disableAutostart();
	}

	function setApp<K extends 'checkUpdatesOnLaunch' | 'maxSnapshots' | 'snapToGrid' | 'gridSize' | 'confirmOverwriteChanges'>(
		key: K,
		value: (typeof appSettings)[K]
	) {
		appSettings[key] = value;
		appSettings.persist();
	}

	async function setLaunchOnStartup(value: boolean): Promise<void> {
		try {
			if (value) {
				await enableAutostart();
			} else {
				await disableAutostart();
			}
			appSettings.launchOnStartup = value;
			appSettings.persist();
		} catch {
			// Leave the mirror unchanged so the toggle reflects the real (unchanged) OS state.
		}
	}

	onMount(() => {
		void appSettings.syncLaunchOnStartup();
	});
</script>

<Header>
	{#snippet left()}
		<div class="flex items-center gap-2">
			<a class:active={page.route.id === '/'} href="/" class="button-header px-4 text-sm">Pipelines</a>
			{#if !isWindows}
				<a class:active={page.route.id === '/virtual-devices'} href="/virtual-devices" class="button-header px-4 text-sm">Virtual devices</a>
			{/if}
			<a class:active={page.route.id === '/wiki'} href="/wiki" class="button-header px-4 text-sm">Wiki</a>
			<a class:active={page.route.id === '/settings'} href="/settings" class="button-header px-4 text-sm">Settings</a>
		</div>
	{/snippet}
</Header>

<div class="h-[calc(100vh-40px)] overflow-y-auto p-8">
	<div class="flex max-w-2xl flex-col gap-8">
		<section class="flex flex-col gap-3">
			<div>
				<h2 class="text-sm font-semibold text-theme">Appearance</h2>
				<p class="text-xs text-neutral-900">System follows your OS setting and changes with it.</p>
			</div>

			<div class="grid grid-cols-3 gap-2">
				{#each THEMES as t (t.value)}
					<button
						type="button"
						onclick={() => ($themeStore = t.value)}
						class={[
							'rounded-xl border px-3 py-2 text-[11px] font-medium transition-colors',
							$themeStore === t.value
								? 'border-neutral-900 bg-neutral-200 text-theme'
								: 'border-neutral-400 bg-neutral-100 text-neutral-1000 hover:bg-neutral-200'
						]}>
						{t.label}
					</button>
				{/each}
			</div>
		</section>

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
						]}>
						<EdgeShapeIcon shape={s.value} animated={edgeSettings.animated} />
						<span class="text-[11px] font-medium">{s.label}</span>
					</button>
				{/each}
			</div>
		</section>

		<section class="flex flex-col gap-2">
			<div>
				<h2 class="text-sm font-semibold text-theme">Rendering</h2>
				<p class="text-xs text-neutral-900">Turn these off first if the canvas drops frames on a large graph.</p>
			</div>

			{#each [{ key: 'animated' as const, label: 'Animate cables', hint: 'Marching dashes along every connection. Runs a CSS animation per edge.' }, { key: 'pins' as const, label: 'Connector pins', hint: 'Draws a plug at each end of a cable.' }] as row (row.key)}
				<Toggle checked={edgeSettings[row.key]} label={row.label} hint={row.hint} onChange={() => toggle(row.key)} />
			{/each}
		</section>

		<section class="flex flex-col gap-2">
			<div>
				<h2 class="text-sm font-semibold text-theme">Canvas</h2>
				<p class="text-xs text-neutral-900">Where nodes land when you drag them.</p>
			</div>

			<Toggle
				checked={appSettings.snapToGrid}
				label="Snap to grid"
				hint="Nodes align to a fixed grid instead of moving freely."
				onChange={() => setApp('snapToGrid', !appSettings.snapToGrid)} />

			{#if appSettings.snapToGrid}
				<div class="flex items-center gap-2 pl-1">
					<span class="text-xs text-neutral-900">Grid size</span>
					{#each GRID_SIZES as size (size)}
						<button
							type="button"
							onclick={() => setApp('gridSize', size)}
							class={[
								'rounded-lg border px-2.5 py-1 font-mono text-[11px] tabular-nums transition-colors',
								appSettings.gridSize === size
									? 'border-neutral-900 bg-neutral-200 text-theme'
									: 'border-neutral-400 bg-neutral-100 text-neutral-1000 hover:bg-neutral-200'
							]}>
							{size}
						</button>
					{/each}
				</div>
			{/if}
		</section>

		<section class="flex flex-col gap-2">
			<div>
				<h2 class="text-sm font-semibold text-theme">History</h2>
				<p class="text-xs text-neutral-900">Snapshots kept per pipeline. Older ones are dropped as new ones arrive.</p>
			</div>

			<div class="flex items-center gap-2">
				{#each SNAPSHOT_LIMITS as limit (limit)}
					<button
						type="button"
						onclick={() => setApp('maxSnapshots', limit)}
						class={[
							'rounded-lg border px-3 py-1 font-mono text-[11px] tabular-nums transition-colors',
							appSettings.maxSnapshots === limit
								? 'border-neutral-900 bg-neutral-200 text-theme'
								: 'border-neutral-400 bg-neutral-100 text-neutral-1000 hover:bg-neutral-200'
						]}>
						{limit}
					</button>
				{/each}
			</div>
		</section>

		<section class="flex flex-col gap-2">
			<div>
				<h2 class="text-sm font-semibold text-theme">Updates</h2>
				<p class="text-xs text-neutral-900">Checking from the menu always works, whatever this is set to.</p>
			</div>

			<Toggle
				checked={appSettings.checkUpdatesOnLaunch}
				label="Check on launch"
				hint="Looks for a new version each time the app starts."
				onChange={() => setApp('checkUpdatesOnLaunch', !appSettings.checkUpdatesOnLaunch)} />
		</section>

		<section class="flex flex-col gap-2">
			<div>
				<h2 class="text-sm font-semibold text-theme">Recording</h2>
				<p class="text-xs text-neutral-900">Guards against accidentally erasing an existing recording in Overwrite mode.</p>
			</div>

			<Toggle
				checked={appSettings.confirmOverwriteChanges}
				label="Confirm changes in Overwrite mode"
				hint="Asks for confirmation before changing format, channels or sample rate of an overwrite recording. Can also be skipped per file."
				onChange={() => setApp('confirmOverwriteChanges', !appSettings.confirmOverwriteChanges)} />
		</section>

		<section class="flex flex-col gap-2">
			<div>
				<h2 class="text-sm font-semibold text-theme">Startup</h2>
				<p class="text-xs text-neutral-900">Registers Splitwave with your OS's login items.</p>
			</div>

			<Toggle
				checked={appSettings.launchOnStartup}
				label="Launch on system startup"
				hint="Starts Splitwave automatically when you log in."
				onChange={() => setLaunchOnStartup(!appSettings.launchOnStartup)} />
		</section>

		<PresetsSection />

		<button type="button" class="button-main primary self-start text-xs" onclick={() => resetAll()}> Reset to defaults </button>
	</div>
</div>
