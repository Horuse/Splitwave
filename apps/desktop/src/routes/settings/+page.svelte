<script lang="ts">
	import { slide } from 'svelte/transition';
	import { Chip, SectionCard, Separator } from '@splitwave/ui';
	import IconPaint from '~icons/fluent/paint-brush-20-filled';
	import IconGrid from '~icons/fluent/grid-20-filled';
	import IconGauge from '~icons/fluent/gauge-20-filled';
	import IconRecord from '~icons/fluent/record-20-filled';
	import IconSync from '~icons/fluent/arrow-sync-20-filled';
	import IconBookmark from '~icons/fluent/bookmark-20-filled';
	import IconReset from '~icons/fluent/arrow-reset-20-filled';
	import { Button, NumberStepper, Toggle } from '@splitwave/ui';
	import { enable as enableAutostart, disable as disableAutostart } from '@tauri-apps/plugin-autostart';
	import { onMount } from 'svelte';
	import Header from '$lib/components/layout/header.svelte';
	import { edgeSettings, type EdgeShape } from '$lib/modules/flow/edge_settings.svelte';
	import EdgeShapeIcon from '$lib/modules/flow/ui/_edge_shape_icon.svelte';
	import { themeStore, type ThemePref } from '$lib/modules/theme/stores';
	import {
		appSettings,
		UI_SCALE_MIN,
		UI_SCALE_MAX,
		UI_SCALE_STEP,
		GRID_SIZES,
		SNAPSHOT_LIMITS,
		PIPELINE_SAMPLE_RATE_PRESETS,
		BUFFER_FRAME_PRESETS
	} from '$lib/modules/settings/stores.svelte';
	import { formatHz, formatLatencyMs } from '@splitwave/nodes';
	import PresetsSection from './_presets_section.svelte';
	import { audioStore } from '$lib/modules/audio/stores.svelte';
	import { methods as trayMethods } from '$lib/modules/tray/methods';
	import { DEFAULT_SAMPLE_RATE, MAX_SAMPLE_RATE, MIN_SAMPLE_RATE } from '@splitwave/nodes/generated/engine';

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
		const rate = appSettings.pipelineSampleRate;
		const frames = appSettings.bufferFrames;
		edgeSettings.reset();
		appSettings.reset();
		void disableAutostart();
		if (appSettings.pipelineSampleRate !== rate || appSettings.bufferFrames !== frames) applyEngineFormat();
	}

	/** Every device reopens at the new format, so a running pipeline takes it now. */
	function applyEngineFormat() {
		audioStore.applyEngineFormat().catch((e) => audioStore.reportError(e));
	}

	function setEngine<K extends 'pipelineSampleRate' | 'bufferFrames'>(key: K, value: (typeof appSettings)[K]) {
		if (appSettings[key] === value) return;
		setApp(key, value);
		applyEngineFormat();
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
			| 'bufferFrames'
			| 'animatedNumbers'
			| 'closeToTray'
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

	let trayAvailable = $state(true);

	onMount(() => {
		void appSettings.syncLaunchOnStartup();
		trayMethods
			.isAvailable()
			.then((available) => (trayAvailable = available))
			.catch((e) => audioStore.reportError(e));
	});
</script>

<Header />

<div class="min-h-0 flex-1 overflow-y-auto">
	<div class="mx-auto flex w-full max-w-3xl flex-col gap-5 p-8">
		<h1 class="text-2xl font-semibold">Settings</h1>

		<SectionCard title="Appearance" description="How Splitwave looks. System follows your OS setting and changes with it." icon={IconPaint}>
			<div class="grid grid-cols-3 gap-2">
				{#each THEMES as t (t.value)}
					<Chip class="py-2" selected={$themeStore === t.value} onclick={() => ($themeStore = t.value)}>{t.label}</Chip>
				{/each}
			</div>

			<div class="flex items-center gap-2">
				<span class="text-xs text-neutral-900">Interface scale</span>
				<NumberStepper
					value={appSettings.uiScale}
					min={UI_SCALE_MIN}
					max={UI_SCALE_MAX}
					step={UI_SCALE_STEP}
					label="Interface scale"
					onchange={(v) => appSettings.setUiScale(v)} />
				<span class="font-mono text-xs text-neutral-800 tabular-nums">%</span>
				<Chip disabled={appSettings.uiScale === 100} onclick={() => appSettings.setUiScale(100)}>Reset</Chip>
			</div>

			<Toggle
				checked={appSettings.animatedNumbers}
				label="Animated numbers"
				hint="Timers and latency roll their digits when they change. Off shows the new value at once."
				onChange={() => setApp('animatedNumbers', !appSettings.animatedNumbers)} />

			<Separator />

			<div class="flex flex-col gap-2">
				<h3 class="text-xs font-semibold text-theme">Cable shape</h3>
				<div class="grid grid-cols-2 gap-2 sm:grid-cols-4">
					{#each SHAPES as sh (sh.value)}
						<Chip class="gap-2 p-3" title={sh.hint} selected={edgeSettings.shape === sh.value} onclick={() => setShape(sh.value)}>
							<EdgeShapeIcon shape={sh.value} animated={edgeSettings.animated} />
							{sh.label}
						</Chip>
					{/each}
				</div>
			</div>

			<div class="flex flex-col gap-2">
				<h3 class="text-xs font-semibold text-theme">Rendering</h3>
				<p class="text-xs text-neutral-900">Turn these off first if the canvas drops frames on a large graph.</p>
				{#each [{ key: 'animated' as const, label: 'Animate cables', hint: 'Marching dashes along every connection. Runs a CSS animation per edge.' }, { key: 'pins' as const, label: 'Connector pins', hint: 'Draws a plug at each end of a cable.' }] as row (row.key)}
					<Toggle checked={edgeSettings[row.key]} label={row.label} hint={row.hint} onChange={() => toggle(row.key)} />
				{/each}
			</div>
		</SectionCard>

		<SectionCard title="Canvas and history" description="Where nodes land when you drag them, and how many snapshots each pipeline keeps." icon={IconGrid}>
			<Toggle
				checked={appSettings.snapToGrid}
				label="Snap to grid"
				hint="Nodes align to a fixed grid instead of moving freely."
				onChange={() => setApp('snapToGrid', !appSettings.snapToGrid)} />

			{#if appSettings.snapToGrid}
				<div class="flex items-center gap-2 pl-1" transition:slide={{ duration: 150 }}>
					<span class="text-xs text-neutral-900">Grid size</span>
					{#each GRID_SIZES as size (size)}
						<Chip mono selected={appSettings.gridSize === size} onclick={() => setApp('gridSize', size)}>{size}</Chip>
					{/each}
				</div>
			{/if}

			<Separator />

			<div class="flex flex-col gap-2">
				<h3 class="text-xs font-semibold text-theme">Snapshots per pipeline</h3>
				<p class="text-xs text-neutral-900">Older ones are dropped as new ones arrive.</p>
				<div class="flex items-center gap-2">
					{#each SNAPSHOT_LIMITS as limit (limit)}
						<Chip mono selected={appSettings.maxSnapshots === limit} onclick={() => setApp('maxSnapshots', limit)}>{limit}</Chip>
					{/each}
				</div>
			</div>
		</SectionCard>

		<SectionCard title="Audio engine" description="Sample rate, buffer size and what happens when a device goes away." icon={IconGauge}>
			<div class="flex flex-col gap-2">
				<h3 class="text-xs font-semibold text-theme">Pipeline sample rate</h3>
				<p class="text-xs text-neutral-900">
					Working sample rate for mixing and DSP. Matching this rate across your input and output devices ensures bit-transparent audio with zero
					resampling artifacts, preserving low-level details and preventing intersample clipping.
				</p>
				<div class="flex flex-wrap items-center gap-2">
					{#each PIPELINE_SAMPLE_RATE_PRESETS as rate (rate)}
						<Chip
							mono
							selected={!customRateSelected && appSettings.pipelineSampleRate === rate}
							onclick={() => {
								customRateSelected = false;
								setEngine('pipelineSampleRate', rate);
							}}>
							{formatHz(rate)}
						</Chip>
					{/each}
					<Chip
						selected={customRateSelected || !PIPELINE_SAMPLE_RATE_PRESETS.includes(appSettings.pipelineSampleRate as any)}
						onclick={() => (customRateSelected = true)}>
						Custom
					</Chip>
				</div>

				{#if customRateSelected || !PIPELINE_SAMPLE_RATE_PRESETS.includes(appSettings.pipelineSampleRate as any)}
					<div class="flex items-center gap-2 pt-1 pl-1" transition:slide={{ duration: 150 }}>
						<span class="text-xs text-neutral-900">Custom frequency</span>
						<NumberStepper
							value={appSettings.pipelineSampleRate}
							min={MIN_SAMPLE_RATE}
							max={MAX_SAMPLE_RATE}
							step={1}
							label="Custom sample rate"
							onchange={(v) =>
								setEngine('pipelineSampleRate', Math.min(Math.max(Math.round(v) || DEFAULT_SAMPLE_RATE, MIN_SAMPLE_RATE), MAX_SAMPLE_RATE))} />
						<span class="font-mono text-xs text-neutral-800 tabular-nums">Hz (step: 1 Hz)</span>
					</div>
				{/if}
			</div>

			<Separator />

			<div class="flex flex-col gap-2">
				<h3 class="text-xs font-semibold text-theme">Buffer size</h3>
				<p class="text-xs text-neutral-900">
					Samples each device callback carries. Smaller buffers cut latency but leave the CPU less time per block; raise it if the latency readout
					reports dropped audio. Nodes that need larger blocks run at their own size and show an hourglass. A running pipeline reopens its devices at
					the new size right away.
				</p>
				<div class="flex flex-wrap items-center gap-2">
					{#each BUFFER_FRAME_PRESETS as frames (frames)}
						<Chip mono selected={appSettings.bufferFrames === frames} onclick={() => setEngine('bufferFrames', frames)}>
							<span>{frames}</span>
							<span class="text-[9px] text-neutral-800">{formatLatencyMs((frames * 1000) / appSettings.pipelineSampleRate)} ms</span>
						</Chip>
					{/each}
				</div>
			</div>

			<Separator />

			<Toggle
				checked={appSettings.keepRunningOnDisconnect}
				label="Keep pipeline running on disconnect"
				hint="Streams silence to outputs and waits for the device to reconnect. When disabled, disconnecting a device stops the pipeline immediately."
				onChange={() => setApp('keepRunningOnDisconnect', !appSettings.keepRunningOnDisconnect)} />
		</SectionCard>

		<SectionCard title="Recording" description="Guards against accidentally erasing an existing recording in Overwrite mode." icon={IconRecord}>
			<Toggle
				checked={appSettings.confirmOverwriteChanges}
				label="Confirm changes in Overwrite mode"
				hint="Asks for confirmation before changing format, channels or sample rate of an overwrite recording. Can also be skipped per file."
				onChange={() => setApp('confirmOverwriteChanges', !appSettings.confirmOverwriteChanges)} />
		</SectionCard>

		<SectionCard title="Updates and startup" description="Checking from the menu always works, whatever this is set to." icon={IconSync}>
			<Toggle
				checked={appSettings.checkUpdatesOnLaunch}
				label="Check on launch"
				hint="Looks for a new version each time the app starts."
				onChange={() => setApp('checkUpdatesOnLaunch', !appSettings.checkUpdatesOnLaunch)} />

			<Toggle
				checked={appSettings.includePreReleases}
				label="Receive pre-release updates (Beta / RC)"
				hint="Download preview versions (beta, RC, alpha) with the newest features and bug fixes before general release."
				onChange={() => setApp('includePreReleases', !appSettings.includePreReleases)} />

			<Separator />

			<Toggle
				checked={appSettings.launchOnStartup}
				label="Launch on system startup"
				hint="Starts Splitwave automatically when you log in."
				onChange={() => setLaunchOnStartup(!appSettings.launchOnStartup)} />

			<Toggle
				checked={trayAvailable && appSettings.closeToTray}
				disabled={!trayAvailable}
				label="Keep running in the tray"
				hint={trayAvailable
					? 'Closing the window hides it and audio keeps playing. Quit from the tray panel. When off, closing the window quits Splitwave.'
					: 'The tray icon is not available on this system, so closing the window quits Splitwave. See the logs for the reason.'}
				onChange={() => setApp('closeToTray', !appSettings.closeToTray)} />
		</SectionCard>

		<SectionCard
			title="Effect presets"
			description="Presets you saved from a node. Built-in ones are not listed, they cannot be changed."
			icon={IconBookmark}>
			<PresetsSection />
		</SectionCard>

		<Button class="self-start" onclick={() => resetAll()}>
			<IconReset class="size-4" />
			Reset to defaults
		</Button>
	</div>
</div>
