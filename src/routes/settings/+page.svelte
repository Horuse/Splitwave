<script lang="ts">
	import { enable as enableAutostart, disable as disableAutostart } from '@tauri-apps/plugin-autostart';
	import { onMount } from 'svelte';
	import Header from '$lib/components/layout/header.svelte';
	import HeaderNav from '$lib/components/layout/header_nav.svelte';
	import { edgeSettings, type EdgeShape } from '$lib/modules/flow/edge_settings.svelte';
	import EdgeShapeIcon from '$lib/modules/flow/ui/_edge_shape_icon.svelte';
	import Toggle from '$lib/components/toggle.svelte';
	import { themeStore, type ThemePref } from '$lib/modules/theme/stores';
	import {
		appSettings,
		UI_SCALE_MIN,
		UI_SCALE_MAX,
		UI_SCALE_STEP,
		GRID_SIZES,
		SNAPSHOT_LIMITS,
		PIPELINE_SAMPLE_RATE_PRESETS
	} from '$lib/modules/settings/stores.svelte';
	import NumberStepper from '$lib/components/number_stepper.svelte';
	import { formatHz } from '$lib/components/format';
	import { LOCALES, t } from '$lib/modules/i18n';
	import PresetsSection from './_presets_section.svelte';

	const SHAPES: { value: EdgeShape; label: string; hint: string }[] = [
		{ value: 'bezier', label: 'settings.shapeBezier', hint: 'settings.shapeBezierHint' },
		{ value: 'smoothstep', label: 'settings.shapeSmoothstep', hint: 'settings.shapeSmoothstepHint' },
		{ value: 'step', label: 'settings.shapeStep', hint: 'settings.shapeStepHint' },
		{ value: 'straight', label: 'settings.shapeStraight', hint: 'settings.shapeStraightHint' }
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
		{ value: 'light', label: 'theme.light' },
		{ value: 'dark', label: 'theme.dark' },
		{ value: 'system', label: 'theme.system' }
	];

	function resetAll() {
		edgeSettings.reset();
		appSettings.reset();
		void disableAutostart();
	}

	let customRateSelected = $state(false);

	function setApp<
		K extends
			| 'checkUpdatesOnLaunch'
			| 'includePreReleases'
			| 'maxSnapshots'
			| 'snapToGrid'
			| 'gridSize'
			| 'confirmOverwriteChanges'
			| 'keepRunningOnDisconnect'
			| 'pipelineSampleRate'
	>(key: K, value: (typeof appSettings)[K]) {
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
		<HeaderNav />
	{/snippet}
</Header>

<div class="min-h-0 flex-1 overflow-y-auto p-8">
	<div class="flex max-w-2xl flex-col gap-8">
		<section class="flex flex-col gap-3">
			<div>
				<h2 class="text-sm font-semibold text-theme">{t('settings.appearance')}</h2>
				<p class="text-xs text-neutral-900">{t('settings.appearanceHint')}</p>
			</div>

			<div class="grid grid-cols-3 gap-2">
				{#each THEMES as theme (theme.value)}
					<button
						type="button"
						onclick={() => ($themeStore = theme.value)}
						class={[
							'rounded-xl border px-3 py-2 text-[11px] font-medium transition-colors',
							$themeStore === theme.value
								? 'border-neutral-900 bg-neutral-200 text-theme'
								: 'border-neutral-400 bg-neutral-100 text-neutral-1000 hover:bg-neutral-200'
						]}>
						{t(theme.label)}
					</button>
				{/each}
			</div>

			<div class="flex items-center gap-2">
				<span class="text-xs text-neutral-900">{t('settings.interfaceScale')}</span>
				<NumberStepper
					value={appSettings.uiScale}
					min={UI_SCALE_MIN}
					max={UI_SCALE_MAX}
					step={UI_SCALE_STEP}
					label={t('settings.interfaceScale')}
					onchange={(v) => appSettings.setUiScale(v)} />
				<span class="font-mono text-xs text-neutral-800 tabular-nums">%</span>
				<button
					type="button"
					disabled={appSettings.uiScale === 100}
					onclick={() => appSettings.setUiScale(100)}
					class="rounded-lg border border-neutral-400 bg-neutral-100 px-3 py-1 text-[11px] font-medium text-neutral-1000 transition-colors hover:bg-neutral-200 disabled:opacity-40">
					{t('common.reset')}
				</button>
			</div>

			<div class="flex flex-col gap-1">
				<span class="text-xs text-neutral-900">{t('settings.language')}</span>
				<div class="grid grid-cols-3 gap-2">
					{#each LOCALES as locale (locale.id)}
						<button
							type="button"
							onclick={() => appSettings.setLanguage(locale.id)}
							class={[
								'rounded-xl border px-3 py-2 text-[11px] font-medium transition-colors',
								appSettings.language === locale.id
									? 'border-neutral-900 bg-neutral-200 text-theme'
									: 'border-neutral-400 bg-neutral-100 text-neutral-1000 hover:bg-neutral-200'
							]}>
							<span lang={locale.id}>{locale.label}</span>
						</button>
					{/each}
				</div>
				<p class="text-xs text-neutral-900">{t('settings.languageHint')}</p>
			</div>
		</section>

		<section class="flex flex-col gap-3">
			<div>
				<h2 class="text-sm font-semibold text-theme">{t('settings.cableShape')}</h2>
				<p class="text-xs text-neutral-900">{t('settings.cableShapeHint')}</p>
			</div>

			<div class="grid grid-cols-2 gap-2 sm:grid-cols-4">
				{#each SHAPES as s (s.value)}
					<button
						type="button"
						onclick={() => setShape(s.value)}
						title={t(s.hint)}
						class={[
							'flex flex-col items-center gap-2 rounded-xl border p-3 transition-colors',
							edgeSettings.shape === s.value
								? 'border-neutral-900 bg-neutral-200 text-theme'
								: 'border-neutral-400 bg-neutral-100 text-neutral-1000 hover:bg-neutral-200'
						]}>
						<EdgeShapeIcon shape={s.value} animated={edgeSettings.animated} />
						<span class="text-[11px] font-medium">{t(s.label)}</span>
					</button>
				{/each}
			</div>
		</section>

		<section class="flex flex-col gap-2">
			<div>
				<h2 class="text-sm font-semibold text-theme">{t('settings.rendering')}</h2>
				<p class="text-xs text-neutral-900">{t('settings.renderingHint')}</p>
			</div>

			{#each [{ key: 'animated' as const, label: 'settings.animateCables', hint: 'settings.animateCablesHint' }, { key: 'pins' as const, label: 'settings.connectorPins', hint: 'settings.connectorPinsHint' }] as row (row.key)}
				<Toggle checked={edgeSettings[row.key]} label={t(row.label)} hint={t(row.hint)} onChange={() => toggle(row.key)} />
			{/each}
		</section>

		<section class="flex flex-col gap-2">
			<div>
				<h2 class="text-sm font-semibold text-theme">{t('settings.canvas')}</h2>
				<p class="text-xs text-neutral-900">{t('settings.canvasHint')}</p>
			</div>

			<Toggle
				checked={appSettings.snapToGrid}
				label={t('settings.snapToGrid')}
				hint={t('settings.snapToGridHint')}
				onChange={() => setApp('snapToGrid', !appSettings.snapToGrid)} />

			{#if appSettings.snapToGrid}
				<div class="flex items-center gap-2 pl-1">
					<span class="text-xs text-neutral-900">{t('settings.gridSize')}</span>
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
				<h2 class="text-sm font-semibold text-theme">{t('settings.history')}</h2>
				<p class="text-xs text-neutral-900">{t('settings.historyHint')}</p>
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
				<h2 class="text-sm font-semibold text-theme">{t('settings.updates')}</h2>
				<p class="text-xs text-neutral-900">{t('settings.updatesHint')}</p>
			</div>

			<Toggle
				checked={appSettings.checkUpdatesOnLaunch}
				label={t('settings.checkOnLaunch')}
				hint={t('settings.checkOnLaunchHint')}
				onChange={() => setApp('checkUpdatesOnLaunch', !appSettings.checkUpdatesOnLaunch)} />

			<Toggle
				checked={appSettings.includePreReleases}
				label={t('settings.preReleases')}
				hint={t('settings.preReleasesHint')}
				onChange={() => setApp('includePreReleases', !appSettings.includePreReleases)} />
		</section>

		<section class="flex flex-col gap-2">
			<div>
				<h2 class="text-sm font-semibold text-theme">{t('settings.recording')}</h2>
				<p class="text-xs text-neutral-900">{t('settings.recordingHint')}</p>
			</div>

			<Toggle
				checked={appSettings.confirmOverwriteChanges}
				label={t('settings.confirmOverwrite')}
				hint={t('settings.confirmOverwriteHint')}
				onChange={() => setApp('confirmOverwriteChanges', !appSettings.confirmOverwriteChanges)} />
		</section>

		<section class="flex flex-col gap-2">
			<div>
				<h2 class="text-sm font-semibold text-theme">{t('settings.pipelineRate')}</h2>
				<p class="text-xs text-neutral-900">{t('settings.pipelineRateHint')}</p>
			</div>

			<div class="flex flex-wrap items-center gap-2">
				{#each PIPELINE_SAMPLE_RATE_PRESETS as rate (rate)}
					<button
						type="button"
						onclick={() => {
							customRateSelected = false;
							setApp('pipelineSampleRate', rate);
						}}
						class={[
							'rounded-lg border px-3 py-1 font-mono text-[11px] tabular-nums transition-colors',
							!customRateSelected && appSettings.pipelineSampleRate === rate
								? 'border-neutral-900 bg-neutral-200 text-theme'
								: 'border-neutral-400 bg-neutral-100 text-neutral-1000 hover:bg-neutral-200'
						]}>
						{formatHz(rate)}
					</button>
				{/each}
				<button
					type="button"
					onclick={() => (customRateSelected = true)}
					class={[
						'rounded-lg border px-3 py-1 text-[11px] font-medium transition-colors',
						customRateSelected || !PIPELINE_SAMPLE_RATE_PRESETS.includes(appSettings.pipelineSampleRate as any)
							? 'border-neutral-900 bg-neutral-200 text-theme'
							: 'border-neutral-400 bg-neutral-100 text-neutral-1000 hover:bg-neutral-200'
					]}>
					{t('common.custom')}
				</button>
			</div>

			{#if customRateSelected || !PIPELINE_SAMPLE_RATE_PRESETS.includes(appSettings.pipelineSampleRate as any)}
				<div class="flex items-center gap-2 pt-1 pl-1">
					<span class="text-xs text-neutral-900">{t('settings.customRate')}</span>
					<NumberStepper
						value={appSettings.pipelineSampleRate}
						min={8000}
						max={384000}
						step={1}
						label={t('settings.customSampleRate')}
						onchange={(v) => setApp('pipelineSampleRate', Math.min(Math.max(Math.round(v) || 48000, 8000), 384000))} />
					<span class="font-mono text-xs text-neutral-800 tabular-nums">{t('settings.customRateHint')}</span>
				</div>
			{/if}
		</section>

		<section class="flex flex-col gap-2">
			<div>
				<h2 class="text-sm font-semibold text-theme">{t('settings.disconnection')}</h2>
				<p class="text-xs text-neutral-900">{t('settings.disconnectionHint')}</p>
			</div>

			<Toggle
				checked={appSettings.keepRunningOnDisconnect}
				label={t('settings.keepRunning')}
				hint={t('settings.keepRunningHint')}
				onChange={() => setApp('keepRunningOnDisconnect', !appSettings.keepRunningOnDisconnect)} />
		</section>

		<section class="flex flex-col gap-2">
			<div>
				<h2 class="text-sm font-semibold text-theme">{t('settings.startup')}</h2>
				<p class="text-xs text-neutral-900">{t('settings.startupHint')}</p>
			</div>

			<Toggle
				checked={appSettings.launchOnStartup}
				label={t('settings.launchOnStartup')}
				hint={t('settings.launchOnStartupHint')}
				onChange={() => setLaunchOnStartup(!appSettings.launchOnStartup)} />
		</section>

		<PresetsSection />

		<button type="button" class="button-main primary self-start text-xs" onclick={() => resetAll()}> {t('settings.resetAll')} </button>
	</div>
</div>
