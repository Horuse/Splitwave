<script lang="ts">
	import { Button, Callout, Card, Chip, Input, NumberStepper, SectionCard, Separator, smoothRadius } from '@splitwave/ui';
	import { createId } from '@paralleldrive/cuid2';
	import { LazyStore } from '@tauri-apps/plugin-store';
	import { methods } from '$lib/modules/audio/methods';
	import type { VirtualDeviceConfig, VirtualDriverStatus } from '$lib/modules/audio/types';
	import Header from '$lib/components/layout/header.svelte';
	import { DriverUpdateBanner } from '$lib/modules/audio/ui';
	import Add from '~icons/fluent/add-24-filled';
	import Delete from '~icons/fluent/delete-16-filled';
	import Plug from '~icons/fluent/plug-connected-16-filled';
	import SoundWave from '~icons/fluent/sound-wave-circle-16-filled';
	import { platform } from '@tauri-apps/plugin-os';
	import { formatHz } from '@splitwave/nodes';
	import { appSettings } from '$lib/modules/settings/stores.svelte';
	import { DEFAULT_SAMPLE_RATE, MAX_SAMPLE_RATE, MIN_SAMPLE_RATE } from '@splitwave/nodes/generated/engine';
	import WindowsVirtualMicrophone from './_windows_virtual_microphone.svelte';

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
				(d.sampleRate ?? DEFAULT_SAMPLE_RATE) === (b[i].sampleRate ?? DEFAULT_SAMPLE_RATE)
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
		devices = [...devices, { id: createId(), name: `Device ${devices.length + 1}`, channels: 2, sampleRate: appSettings.pipelineSampleRate }];
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
		const clamped = Math.min(Math.max(Math.round(sampleRate) || DEFAULT_SAMPLE_RATE, MIN_SAMPLE_RATE), MAX_SAMPLE_RATE);
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

<Header />

<div class="min-h-0 flex-1 overflow-y-auto">
	<div class="mx-auto flex w-full max-w-3xl flex-col gap-5 p-8">
		<div class="flex flex-col gap-1">
			<h1 class="text-2xl font-semibold">Virtual devices</h1>
			{#if isWindows}
				<p class="text-sm text-neutral-900">
					Windows uses the fixed VB-CABLE device pair. Send a Speaker node to CABLE Input, then select CABLE Output as the microphone in another app.
				</p>
			{:else}
				<p class="text-sm text-neutral-900">
					A virtual device is a system audio device that exists only in software - no hardware required. Apps can send audio to it or record from it
					just like a real microphone or speaker. Each device appears as both an input and an output, so a pipeline can receive audio from one app and
					route it to another.
				</p>
			{/if}
		</div>

		{#if isWindows}
			<WindowsVirtualMicrophone />
		{:else}
			{#if !isLinux}
				<SectionCard
					title="Audio Server Plugin"
					description="{status?.installed ? 'Installed' : 'Not installed'}. Required for virtual devices to appear in system audio."
					icon={Plug}>
					{#snippet actions()}
						{#if status?.installed}
							<Button tone="danger" onclick={uninstall}>Uninstall</Button>
						{:else}
							<Button tone="brand" onclick={installDriver} disabled={installing}>
								{installing ? 'Installing...' : 'Install'}
							</Button>
						{/if}
					{/snippet}
				</SectionCard>

				<DriverUpdateBanner
					onUpdated={(s) => {
						status = s;
						if (devices.length > 0) appliedDevices = [];
					}} />
			{/if}

			{#if isLinux || status?.installed}
				<div class="flex items-center justify-between pt-2">
					<h2 class="text-lg font-semibold">Devices</h2>
					<Button onclick={addDevice}>
						<Add class="size-4" />
						Add device
					</Button>
				</div>

				{#if devices.length === 0}
					<Card dashed class="flex flex-col items-center gap-3 py-12">
						<SoundWave class="size-10 text-neutral-600" />
						<p class="text-sm text-neutral-900">No virtual devices. Add one above.</p>
					</Card>
				{:else}
					{#each devices as d (d.id)}
						<Card class="flex flex-col gap-4 p-5">
							<div class="flex items-center gap-3">
								<span class="section-card-icon" {@attach smoothRadius({ variable: '--r' })}><SoundWave /></span>
								<Input
									size="md"
									class="flex-1 font-medium"
									value={d.name}
									oninput={(e) => rename(d.id, (e.currentTarget as HTMLInputElement).value)} />
								<span class="rounded-md bg-(--lv-2) px-2 py-1 font-mono text-xs text-neutral-900 tabular-nums">{d.id.slice(0, 8)}</span>
								<Button tone="danger" icon onclick={() => removeDevice(d.id)} aria-label="Remove device" title="Remove device">
									<Delete class="size-4" />
								</Button>
							</div>

							<Separator />

							<div class="flex flex-wrap items-center gap-x-4 gap-y-2">
								<span class="w-24 text-xs text-neutral-900">Channels</span>
								<NumberStepper
									value={d.channels ?? 2}
									min={1}
									max={maxChannels}
									step={1}
									label="Channels"
									onchange={(v) => setChannels(d.id, v)} />
								<div class="flex items-center gap-1">
									{#each [2, 8, 16, 32, 64] as preset (preset)}
										<Chip mono selected={(d.channels ?? 2) === preset} onclick={() => setChannels(d.id, preset)}>{preset}</Chip>
									{/each}
								</div>
								<span class="ml-auto text-[11px] text-neutral-800">Input + output, up to {maxChannels} channels</span>
							</div>

							<div class="flex flex-wrap items-center gap-x-4 gap-y-2">
								<span class="w-24 text-xs text-neutral-900">Sample rate</span>
								<NumberStepper
									value={d.sampleRate ?? DEFAULT_SAMPLE_RATE}
									min={8000}
									max={384000}
									step={1}
									label="Sample rate"
									onchange={(v) => setSampleRate(d.id, v)} />
								<div class="flex items-center gap-1">
									{#each [44100, 48000, 88200, 96000, 192000] as preset (preset)}
										<Chip mono selected={(d.sampleRate ?? DEFAULT_SAMPLE_RATE) === preset} onclick={() => setSampleRate(d.id, preset)}>
											{formatHz(preset)}
										</Chip>
									{/each}
								</div>
								<span class="ml-auto text-[11px] text-neutral-800">Match the project rate to avoid resampling</span>
							</div>
						</Card>
					{/each}
				{/if}

				{#if dirty}
					<Callout>
						<strong>Changes not applied</strong>
						Press Apply to update the system audio devices.
					</Callout>
				{/if}

				{#if error}
					<Callout tone="danger">{error}</Callout>
				{/if}

				<Button tone="brand" class="self-start" onclick={apply} disabled={applying}>
					{applying ? 'Applying...' : 'Apply'}
				</Button>
			{/if}
		{/if}
	</div>
</div>
