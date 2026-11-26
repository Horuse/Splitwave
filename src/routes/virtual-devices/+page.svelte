<script lang="ts">
	import { createId } from '@paralleldrive/cuid2';
	import { LazyStore } from '@tauri-apps/plugin-store';
	import { methods } from '$lib/modules/audio/methods';
	import type { VirtualDeviceConfig, VirtualDriverStatus } from '$lib/modules/audio/types';
	import Header from '$lib/components/layout/header.svelte';
	import HeaderNav from '$lib/components/layout/header_nav.svelte';
	import { DriverUpdateBanner } from '$lib/modules/audio/ui';
	import { Add, Delete, Plug, SoundWave } from '$lib/components/icons';
	import NumberStepper from '$lib/components/number_stepper.svelte';
	import { platform } from '@tauri-apps/plugin-os';
	import { formatHz } from '$lib/components/format';
	import WindowsVirtualMicrophone from './_windows_virtual_microphone.svelte';
	import { t } from '$lib/modules/i18n';

	const isLinux = platform() === 'linux';
	const isWindows = platform() === 'windows';
	const maxChannels = isLinux ? 64 : 256;

	const store = new LazyStore('virtual-devices.json');
	const STORE_KEY = 'devices';

	let devices = $state<VirtualDeviceConfig[]>([]);
	let appliedDevices = $state<VirtualDeviceConfig[]>([]);
	let status = $state<VirtualDriverStatus | null>(null);
	let applying = $state(false);
	let installing = $state(false);
	let error = $state<string | null>(null);

	$effect(() => {
		if (!isWindows) void loadAll();
	});

	function sameDevices(a: VirtualDeviceConfig[], b: VirtualDeviceConfig[]): boolean {
		if (a.length !== b.length) return false;
		return a.every(
			(d, i) =>
				d.id === b[i].id &&
				d.name === b[i].name &&
				(d.channels ?? 2) === (b[i].channels ?? 2) &&
				(d.sampleRate ?? 48_000) === (b[i].sampleRate ?? 48_000)
		);
	}

	let dirty = $derived(!sameDevices(devices, appliedDevices));

	async function loadAll() {
		const [s, saved] = await Promise.all([methods.virtualDriverStatus(), store.get<VirtualDeviceConfig[]>(STORE_KEY)]);
		status = s;
		devices = saved ?? [];
		appliedDevices = saved ?? [];
	}

	function addDevice() {
		// Names are persisted and shown by the OS, so derive the number from the
		// devices that already exist instead of the array length (deleting a
		// middle device would otherwise reuse a name).
		const used = new Set(devices.map((d) => d.name));
		let n = devices.length + 1;
		while (used.has(t('vdev.defaultName', { n }))) n += 1;
		devices = [...devices, { id: createId(), name: t('vdev.defaultName', { n }), channels: 2, sampleRate: 48_000 }];
	}

	function removeDevice(id: string) {
		devices = devices.filter((d) => d.id !== id);
	}

	function rename(id: string, name: string) {
		devices = devices.map((d) => (d.id === id ? { ...d, name } : d));
	}

	function setChannels(id: string, channels: number) {
		const clamped = Math.min(Math.max(Math.round(channels) || 2, 1), maxChannels);
		devices = devices.map((d) => (d.id === id ? { ...d, channels: clamped } : d));
	}

	function setSampleRate(id: string, sampleRate: number) {
		const clamped = Math.min(Math.max(Math.round(sampleRate) || 48_000, 8_000), 384_000);
		devices = devices.map((d) => (d.id === id ? { ...d, sampleRate: clamped } : d));
	}

	async function apply() {
		error = null;
		applying = true;
		try {
			await store.set(STORE_KEY, devices);
			await store.save();
			await methods.applyVirtualDevices(devices);
			appliedDevices = devices;
		} catch (e) {
			error = String(e);
		} finally {
			applying = false;
		}
	}

	async function installDriver() {
		error = null;
		installing = true;
		try {
			await methods.installVirtualDriver();
			status = await methods.virtualDriverStatus();
			// a fresh install has no device config yet; Apply writes it
			if (devices.length > 0) appliedDevices = [];
		} catch (e) {
			error = String(e);
		} finally {
			installing = false;
		}
	}

	async function uninstall() {
		error = null;
		try {
			await methods.uninstallVirtualDriver();
			status = await methods.virtualDriverStatus();
		} catch (e) {
			error = String(e);
		}
	}
</script>

<Header>
	{#snippet left()}
		<HeaderNav />
	{/snippet}
</Header>

<div class="flex min-h-0 flex-1 flex-col gap-8 overflow-y-auto p-8">
	<div class="mt-2 flex flex-col gap-1">
		<h1 class="text-2xl font-semibold">{t('vdev.title')}</h1>

		{#if isWindows}
			<p class="max-w-3xl text-base text-neutral-700">
				{t('vdev.windowsHint')}
			</p>
		{:else}
			<p class="max-w-3xl text-base text-neutral-700">
				{t('vdev.genericHint')}
			</p>
		{/if}
	</div>

	{#if isWindows}
		<WindowsVirtualMicrophone />
	{:else}
		{#if !isLinux}
			<div class="flex items-center gap-4 rounded-2xl bg-neutral-200 px-4 py-4">
				<div class="flex size-9 shrink-0 items-center justify-center rounded-xl bg-neutral-300">
					<Plug class={['size-4.5', status?.installed ? 'text-emerald-600 dark:text-emerald-400' : 'text-neutral-700']} />
				</div>
				<div class="flex-1">
					<div class="font-medium">{t('vdev.audioServerPlugin')}</div>
					<div class="text-xs text-neutral-900">
						{status?.installed ? t('vdev.installed') : t('vdev.notInstalled')} &mdash; {t('vdev.requiredHint')}
					</div>
				</div>
				{#if status?.installed}
					<button class="btn-alert h-full py-1.5" onclick={uninstall}>{t('vdev.uninstall')}</button>
				{:else}
					<button class="btn-alert h-full py-1.5" onclick={installDriver} disabled={installing}>
						{installing ? t('common.installing') : t('common.install')}
					</button>
				{/if}
			</div>

			<DriverUpdateBanner
				onUpdated={(s) => {
					status = s;
					if (devices.length > 0) appliedDevices = [];
				}} />
		{/if}

		{#if isLinux || status?.installed}
			<div class="flex flex-col gap-4">
				<div class="flex items-center justify-between">
					<h2 class="text-lg font-medium">{t('vdev.devices')}</h2>
					<button class="button-main primary gap-1.5 py-1.5" onclick={addDevice}>
						<Add class="size-4" />
						{t('vdev.addDevice')}
					</button>
				</div>

				{#if devices.length === 0}
					<div class="flex flex-col items-center gap-3 rounded-2xl border border-dashed border-neutral-400 py-12">
						<SoundWave class="size-10 text-neutral-600" />
						<p class="text-sm text-neutral-900">{t('vdev.empty')}</p>
					</div>
				{:else}
					<ul class="flex flex-col gap-2">
						{#each devices as d (d.id)}
							<li class="flex flex-col gap-3 rounded-2xl bg-neutral-200 p-4">
								<div class="flex items-center gap-3">
									<div class="flex size-9 shrink-0 items-center justify-center rounded-xl bg-neutral-300">
										<SoundWave class="size-4.5 text-sky-600 dark:text-sky-400" />
									</div>
									<input
										class="input-base h-8 flex-1 font-medium"
										value={d.name}
										oninput={(e) => rename(d.id, (e.currentTarget as HTMLInputElement).value)} />
									<span class="rounded-md bg-neutral-300 px-2 py-1 font-mono text-xs text-neutral-900 tabular-nums">
										{d.id.slice(0, 8)}
									</span>
									<button
										class="btn-alert px-2.5 py-2"
										onclick={() => removeDevice(d.id)}
										aria-label={t('vdev.removeAria')}
										title={t('vdev.removeAria')}>
										<Delete class="size-4" />
									</button>
								</div>
								<div class="flex flex-wrap items-center gap-x-4 gap-y-2">
									<div class="flex items-center gap-2">
										<span class="text-xs text-neutral-900">{t('vdev.channels')}</span>
										<NumberStepper
											value={d.channels ?? 2}
											min={1}
											max={maxChannels}
											step={1}
											label={t('vdev.channels')}
											onchange={(v) => setChannels(d.id, v)} />
									</div>
									<div class="flex items-center gap-1">
										{#each [2, 8, 16, 32, 64] as preset (preset)}
											<button
												class={[
													'rounded-md border px-2 py-0.5 font-mono text-xs tabular-nums transition-colors',
													(d.channels ?? 2) === preset
														? 'border-neutral-800 bg-neutral-600 text-theme'
														: 'border-neutral-400 bg-neutral-100 text-neutral-900 hover:bg-neutral-300'
												]}
												onclick={() => setChannels(d.id, preset)}>
												{preset}
											</button>
										{/each}
									</div>
									<span class="ml-auto text-[11px] text-neutral-800"> {t('vdev.appearsAs', { n: maxChannels })} </span>
								</div>
								<div class="flex flex-wrap items-center gap-x-4 gap-y-2 border-t border-neutral-300/40 pt-2">
									<div class="flex items-center gap-2">
										<span class="text-xs text-neutral-900">{t('vdev.sampleRate')}</span>
										<NumberStepper
											value={d.sampleRate ?? 48_000}
											min={8000}
											max={384000}
											step={1}
											label={t('vdev.sampleRate')}
											onchange={(v) => setSampleRate(d.id, v)} />
										<span class="font-mono text-xs text-neutral-800 tabular-nums">Hz</span>
									</div>
									<div class="flex items-center gap-1">
										{#each [44100, 48000, 88200, 96000, 192000] as preset (preset)}
											<button
												class={[
													'rounded-md border px-2 py-0.5 font-mono text-xs tabular-nums transition-colors',
													(d.sampleRate ?? 48_000) === preset
														? 'border-neutral-800 bg-neutral-600 text-theme'
														: 'border-neutral-400 bg-neutral-100 text-neutral-900 hover:bg-neutral-300'
												]}
												onclick={() => setSampleRate(d.id, preset)}>
												{formatHz(preset)}
											</button>
										{/each}
									</div>
									<span class="ml-auto text-[11px] text-neutral-800"> {t('vdev.matchesProject')} </span>
								</div>
							</li>
						{/each}
					</ul>
				{/if}
			</div>

			{#if dirty}
				<div class="warning-block">
					<strong>{t('vdev.changesNotApplied')}</strong>
					{t('vdev.applyHint')}
				</div>
			{/if}

			{#if error}
				<p class="text-sm text-red-500">{error}</p>
			{/if}

			<div class="flex items-center gap-4">
				<button class="button-main primary px-8 py-2" onclick={apply} disabled={applying}>
					{applying ? t('common.applying') : t('common.apply')}
				</button>
			</div>
		{/if}
	{/if}
</div>
