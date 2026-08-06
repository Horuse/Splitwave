<script lang="ts">
	import { createId } from '@paralleldrive/cuid2';
	import { LazyStore } from '@tauri-apps/plugin-store';
	import { methods } from '$lib/modules/audio/methods';
	import type { VirtualDeviceConfig, VirtualDriverStatus } from '$lib/modules/audio/types';
	import Header from '$lib/components/layout/header.svelte';
	import { DriverUpdateBanner } from '$lib/modules/audio/ui';
	import { Add, Delete, Plug, SoundWave } from '$lib/components/icons';
	import { page } from '$app/state';
	import { platform } from '@tauri-apps/plugin-os';

	const isLinux = platform() === 'linux';
	// Virtual devices need a kernel driver on Windows; unsupported there.
	const isWindows = platform() === 'windows';

	const store = new LazyStore('virtual-devices.json');
	const STORE_KEY = 'devices';
	const DEFAULT_SAMPLE_RATE = 48_000;
	const SAMPLE_RATE_PRESETS = [44_100, 48_000, 88_200, 96_000, 192_000];

	let devices = $state<VirtualDeviceConfig[]>([]);
	let status = $state<VirtualDriverStatus | null>(null);
	let applying = $state(false);
	let installing = $state(false);
	let error = $state<string | null>(null);
	let dirty = $state(false);

	$effect(() => {
		loadAll();
	});

	async function loadAll() {
		const [s, saved] = await Promise.all([
			methods.virtualDriverStatus(),
			store.get<VirtualDeviceConfig[]>(STORE_KEY)
		]);
		status = s;
		devices = (saved ?? []).map(normalizeDevice);
		dirty = false;
	}

	function normalizeDevice(d: VirtualDeviceConfig): VirtualDeviceConfig {
		return {
			...d,
			channels: d.channels ?? 2,
			sampleRate: d.sampleRate ?? DEFAULT_SAMPLE_RATE
		};
	}

	function addDevice() {
		devices = [
			...devices,
			{
				id: createId(),
				name: `Device ${devices.length + 1}`,
				channels: 2,
				sampleRate: DEFAULT_SAMPLE_RATE
			}
		];
		dirty = true;
	}

	function removeDevice(id: string) {
		devices = devices.filter((d) => d.id !== id);
		dirty = true;
	}

	function rename(id: string, name: string) {
		devices = devices.map((d) => (d.id === id ? { ...d, name } : d));
		dirty = true;
	}

	function setChannels(id: string, channels: number) {
		const clamped = Math.min(Math.max(Math.round(channels) || 2, 1), 256);
		devices = devices.map((d) => (d.id === id ? { ...d, channels: clamped } : d));
		dirty = true;
	}

	function setSampleRate(id: string, sampleRate: number) {
		const clamped = Math.min(
			Math.max(Math.round(sampleRate) || DEFAULT_SAMPLE_RATE, 8_000),
			384_000
		);
		devices = devices.map((d) => (d.id === id ? { ...d, sampleRate: clamped } : d));
		dirty = true;
	}

	async function apply() {
		error = null;
		applying = true;
		try {
			await store.set(STORE_KEY, devices);
			await store.save();
			await methods.applyVirtualDevices(devices);
			dirty = false;
		} catch (e) {
			error = String(e);
		} finally {
			applying = false;
		}
	}

	async function install() {
		error = null;
		installing = true;
		try {
			await methods.installVirtualDriver();
			status = await methods.virtualDriverStatus();
			// a fresh install has no device config yet; Apply writes it
			if (devices.length > 0) dirty = true;
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
		<div class="flex items-center gap-2">
			<a class:active={page.route.id === '/'} href="/" class="button-header px-4 text-sm"
				>Pipelines</a
			>
			{#if !isWindows}
				<a
					class:active={page.route.id === '/virtual-devices'}
					href="/virtual-devices"
					class="button-header px-4 text-sm">Virtual devices</a
				>
			{/if}

			<a
				class:active={page.route.id === '/wiki'}
				href="/wiki"
				class="button-header px-4 text-sm">Wiki</a
			>
			<a
				class:active={page.route.id === '/settings'}
				href="/settings"
				class="button-header px-4 text-sm">Settings</a
			>
		</div>
	{/snippet}
</Header>

<div class="flex h-[calc(100vh-40px)] flex-col gap-8 overflow-y-auto p-8">
	<div class="mt-2 flex flex-col gap-1">
		<h1 class="text-2xl font-semibold">Virtual Devices</h1>

		<p class="max-w-2xl text-sm text-neutral-700">
			A virtual device is a system audio device that exists only in software — no hardware
			required. Apps can send audio to it or record from it just like a real microphone or
			speaker. Each device appears as both an input and an output, so a pipeline can receive
			audio from one app and route it to another.
		</p>
	</div>

	{#if isWindows}
		<p class="text-sm text-theme">Virtual devices are not supported on Windows.</p>
	{/if}

	{#if !isLinux && !isWindows}
		<div class="flex items-center gap-4 rounded-2xl bg-neutral-200 px-4 py-4">
			<div class="flex size-9 shrink-0 items-center justify-center rounded-xl bg-neutral-300">
				<Plug
					class={[
						'size-4.5',
						status?.installed
							? 'text-emerald-600 dark:text-emerald-400'
							: 'text-neutral-700'
					]}
				/>
			</div>
			<div class="flex-1">
				<div class="font-medium">Audio Server Plugin</div>
				<div class="text-xs text-neutral-900">
					{status?.installed ? 'Installed' : 'Not installed'} &mdash; required for virtual devices
					to appear in system audio
				</div>
			</div>
			{#if status?.installed}
				<button class="btn-alert h-full py-1.5" onclick={uninstall}>Uninstall</button>
			{:else}
				<button class="btn-alert h-full py-1.5" onclick={install} disabled={installing}>
					{installing ? 'Installing...' : 'Install'}
				</button>
			{/if}
		</div>

		<DriverUpdateBanner
			onUpdated={(s) => {
				status = s;
				if (devices.length > 0) dirty = true;
			}}
		/>
	{/if}

	{#if isLinux || status?.installed}
		<div class="flex flex-col gap-4">
			<div class="flex items-center justify-between">
				<h2 class="text-lg font-medium">Devices</h2>
				<button class="button-main primary gap-1.5 py-1.5" onclick={addDevice}>
					<Add class="size-4" />
					Add device
				</button>
			</div>

			{#if devices.length === 0}
				<div
					class="flex flex-col items-center gap-3 rounded-2xl border border-dashed border-neutral-400 py-12"
				>
					<SoundWave class="size-10 text-neutral-600" />
					<p class="text-sm text-neutral-900">No virtual devices. Add one above.</p>
				</div>
			{:else}
				<ul class="flex flex-col gap-2">
					{#each devices as d (d.id)}
						<li class="flex flex-col gap-3 rounded-2xl bg-neutral-200 p-4">
							<div class="flex items-center gap-3">
								<div
									class="flex size-9 shrink-0 items-center justify-center rounded-xl bg-neutral-300"
								>
									<SoundWave class="size-4.5 text-sky-600 dark:text-sky-400" />
								</div>
								<input
									class="input-base h-8 flex-1 font-medium"
									value={d.name}
									oninput={(e) =>
										rename(d.id, (e.currentTarget as HTMLInputElement).value)}
								/>
								<span
									class="rounded-md bg-neutral-300 px-2 py-1 font-mono text-xs text-neutral-900 tabular-nums"
								>
									{d.id.slice(0, 8)}
								</span>
								<button
									class="btn-alert px-2.5 py-2"
									onclick={() => removeDevice(d.id)}
									aria-label="Remove device"
									title="Remove device"
								>
									<Delete class="size-4" />
								</button>
							</div>
							<div class="flex flex-wrap items-center gap-x-4 gap-y-2">
								<div class="flex items-center gap-2">
									<span class="text-xs text-neutral-900">Channels</span>
									<div
										class="flex items-center overflow-hidden rounded-lg border border-neutral-400 bg-neutral-100"
									>
										<button
											class="flex h-7 w-7 items-center justify-center text-neutral-900 hover:bg-neutral-300 disabled:opacity-40"
											disabled={(d.channels ?? 2) <= 1}
											onclick={() => setChannels(d.id, (d.channels ?? 2) - 1)}
											aria-label="Fewer channels"
										>
											&minus;
										</button>
										<input
											class="h-7 w-14 [appearance:textfield] border-x border-neutral-400 bg-transparent text-center font-mono text-sm tabular-nums outline-none [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none"
											type="number"
											min="1"
											max="256"
											value={d.channels ?? 2}
											onchange={(e) =>
												setChannels(
													d.id,
													(e.currentTarget as HTMLInputElement)
														.valueAsNumber
												)}
										/>
										<button
											class="flex h-7 w-7 items-center justify-center text-neutral-900 hover:bg-neutral-300 disabled:opacity-40"
											disabled={(d.channels ?? 2) >= 256}
											onclick={() => setChannels(d.id, (d.channels ?? 2) + 1)}
											aria-label="More channels"
										>
											+
										</button>
									</div>
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
											onclick={() => setChannels(d.id, preset)}
										>
											{preset}
										</button>
									{/each}
								</div>
								<span class="ml-auto text-[11px] text-neutral-800">
									Appears as input + output &middot; up to 256 channels
								</span>
							</div>
							<div class="flex flex-wrap items-center gap-x-4 gap-y-2">
								<div class="flex items-center gap-2">
									<span class="text-xs text-neutral-900">Sample rate</span>
									<div
										class="flex items-center overflow-hidden rounded-lg border border-neutral-400 bg-neutral-100"
									>
										<input
											class="h-7 w-24 [appearance:textfield] bg-transparent text-center font-mono text-sm tabular-nums outline-none [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none"
											type="number"
											min="8000"
											max="384000"
											step="100"
											value={d.sampleRate ?? DEFAULT_SAMPLE_RATE}
											onchange={(e) =>
												setSampleRate(
													d.id,
													(e.currentTarget as HTMLInputElement)
														.valueAsNumber
												)}
										/>
										<span
											class="border-l border-neutral-400 px-2 font-mono text-xs text-neutral-900"
										>
											Hz
										</span>
									</div>
								</div>
								<div class="flex items-center gap-1">
									{#each SAMPLE_RATE_PRESETS as preset (preset)}
										<button
											class={[
												'rounded-md border px-2 py-0.5 font-mono text-xs tabular-nums transition-colors',
												(d.sampleRate ?? DEFAULT_SAMPLE_RATE) === preset
													? 'border-neutral-800 bg-neutral-600 text-theme'
													: 'border-neutral-400 bg-neutral-100 text-neutral-900 hover:bg-neutral-300'
											]}
											onclick={() => setSampleRate(d.id, preset)}
										>
											{preset >= 1000 ? `${preset / 1000}k` : preset}
										</button>
									{/each}
								</div>
							</div>
						</li>
					{/each}
				</ul>
			{/if}
		</div>

		{#if dirty}
			<div class="warning-block">
				<strong>Changes not applied</strong>
				Press Apply to update the system audio devices.
			</div>
		{/if}

		{#if error}
			<p class="text-sm text-red-500">{error}</p>
		{/if}

		<div class="flex items-center gap-4">
			<button class="button-main primary px-8 py-2" onclick={apply} disabled={applying}>
				{applying ? 'Applying...' : 'Apply'}
			</button>
		</div>
	{/if}
</div>
