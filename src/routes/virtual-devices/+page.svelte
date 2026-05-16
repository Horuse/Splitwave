<script lang="ts">
	import { createId } from '@paralleldrive/cuid2';
	import { LazyStore } from '@tauri-apps/plugin-store';
	import { methods } from '$lib/modules/audio/methods';
	import type { VirtualDeviceConfig, VirtualDriverStatus } from '$lib/modules/audio/types';
	import Header from '$lib/components/layout/header.svelte';
	import { page } from '$app/state';

	const store = new LazyStore('virtual-devices.json');
	const STORE_KEY = 'devices';

	let devices = $state<VirtualDeviceConfig[]>([]);
	let status = $state<VirtualDriverStatus | null>(null);
	let applying = $state(false);
	let installing = $state(false);
	let error = $state<string | null>(null);
	let applied = $state(false);

	$effect(() => {
		loadAll();
	});

	async function loadAll() {
		const [s, saved] = await Promise.all([
			methods.virtualDriverStatus(),
			store.get<VirtualDeviceConfig[]>(STORE_KEY)
		]);
		status = s;
		devices = saved ?? [];
	}

	function addDevice() {
		devices = [...devices, { id: createId(), name: `Device ${devices.length + 1}` }];
		applied = false;
	}

	function removeDevice(id: string) {
		devices = devices.filter((d) => d.id !== id);
		applied = false;
	}

	function rename(id: string, name: string) {
		devices = devices.map((d) => (d.id === id ? { ...d, name } : d));
		applied = false;
	}

	async function apply() {
		error = null;
		applied = false;
		applying = true;
		try {
			await store.set(STORE_KEY, devices);
			await store.save();
			await methods.applyVirtualDevices(devices);
			applied = true;
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
			<a class:active={page.route.id === '/'} href="/" class="button-header px-4 text-sm">Pipelines</a>
			<a class:active={page.route.id === '/virtual-devices'} href="/virtual-devices" class="button-header px-4 text-sm">Virtual devices</a>
		</div>
	{/snippet}
</Header>

<div class="flex flex-col gap-8 p-8">
	<div class="h-10.5 flex items-center">
		<h1 class="text-2xl font-semibold ">Virtual Devices</h1>
	</div>

	<div class="flex items-center gap-4 rounded-2xl bg-neutral-200 px-4 py-4">
		<div class="flex-1">
			<div class="font-medium">Audio Server Plugin</div>
			<div class="text-xs text-neutral-900">
				{status?.installed ? 'Installed' : 'Not installed'} &mdash; required for virtual devices to
				appear in system audio
			</div>
		</div>
		{#if status?.installed}
			<button class="button-main red py-1.5" onclick={uninstall}>Uninstall</button>
		{:else}
			<button
				class="button-main secondary py-1.5"
				onclick={install}
				disabled={installing}
			>
				{installing ? 'Installing...' : 'Install'}
			</button>
		{/if}
	</div>

	{#if status?.installed}
		<div class="flex flex-col gap-4">
			<div class="flex items-center justify-between">
				<h2 class="text-lg font-medium">Devices</h2>
				<button class="button-main primary py-1.5" onclick={addDevice}>Add device</button>
			</div>

			{#if devices.length === 0}
				<p class="text-sm text-theme">No virtual devices. Add one above.</p>
			{:else}
				<ul class="flex flex-col gap-2">
					{#each devices as d (d.id)}
						<li class="flex items-center gap-3 rounded-2xl bg-neutral-200 px-4 py-3">
							<input
								class="flex-1 bg-transparent font-medium outline-none"
								value={d.name}
								oninput={(e) => rename(d.id, (e.currentTarget as HTMLInputElement).value)}
							/>
							<span class="font-mono tabular-nums text-xs text-neutral-900"
								>{d.id.slice(0, 8)}</span
							>
							<button
								class="button-main red py-1"
								onclick={() => removeDevice(d.id)}
								aria-label="Remove device"
							>
								Remove
							</button>
						</li>
					{/each}
				</ul>
			{/if}
		</div>

		{#if error}
			<p class="text-sm text-red-500">{error}</p>
		{/if}

		<div class="flex items-center gap-4">
			<button class="button-main primary px-8 py-2" onclick={apply} disabled={applying}>
				{applying ? 'Applying...' : 'Apply'}
			</button>
			{#if applied}
				<span class="text-sm text-green-600">Applied</span>
			{:else}
				<span class="text-xs text-neutral-900">Audio will briefly interrupt when applied.</span>
			{/if}
		</div>
	{/if}
</div>
