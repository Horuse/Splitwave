<script lang="ts">
	import { openUrl } from '@tauri-apps/plugin-opener';
	import { onMount } from 'svelte';
	import { Plug } from '$lib/components/icons';
	import { methods } from '$lib/modules/audio/methods';
	import type { WindowsVirtualCableStatus } from '$lib/modules/audio/types';

	let status = $state<WindowsVirtualCableStatus | null>(null);
	let checking = $state(true);
	let installing = $state(false);
	let issue = $state<string | null>(null);

	const needsRestart = $derived(status?.state === 'rebootRequired' || status?.state === 'removalPendingReboot');
	const incomplete = $derived(status?.state === 'partial');
	const ready = $derived(!!status?.usable && !!status.renderEndpointName && !!status.captureEndpointName);

	function errorCode(value: unknown): string | null {
		if (!value || typeof value !== 'object' || !('code' in value)) return null;
		return typeof value.code === 'string' ? value.code : null;
	}

	function errorMessage(value: unknown): string | null {
		if (!value || typeof value !== 'object' || !('message' in value)) return null;
		return typeof value.message === 'string' && value.message.trim() ? value.message : null;
	}

	function friendlyIssue(value: unknown): string {
		switch (errorCode(value)) {
			case 'elevationCancelled':
				return 'Administrator approval was cancelled.';
			case 'downloadFailed':
				return 'Splitwave could not download VB-CABLE. Check your connection and try again.';
			case 'checksumMismatch':
			case 'invalidSignature':
				return 'The downloaded driver could not be verified, so Splitwave did not run it.';
			case 'driverPackageNotDetected':
				return 'VB-CABLE installer finished, but Windows did not register the expected driver.';
			case 'helperResultMissing':
			case 'helperResultInvalid':
				return 'Splitwave could not read the installer result. Details were written to the app log.';
			case 'operationInProgress':
				return 'Another virtual microphone operation is already in progress.';
			default:
				return errorMessage(value) ?? 'The virtual microphone could not be installed.';
		}
	}

	async function loadStatus(showLoading = status === null) {
		if (installing) return;
		checking = showLoading;
		try {
			status = await methods.windowsVirtualCableStatus();
			issue = null;
		} catch (error) {
			issue = errorMessage(error) ?? 'Splitwave could not check the Windows virtual microphone status.';
		} finally {
			checking = false;
		}
	}

	async function install() {
		issue = null;
		installing = true;
		try {
			status = await methods.installWindowsVirtualCable();
		} catch (error) {
			issue = friendlyIssue(error);
			try {
				status = await methods.windowsVirtualCableStatus();
				if (status.usable) issue = null;
			} catch {
				// Keep the installation error when the follow-up probe also fails.
			}
		} finally {
			installing = false;
		}
	}

	async function learnAboutVbCable() {
		try {
			await openUrl('https://vb-audio.com/Cable/');
		} catch {
			issue = 'Splitwave could not open the VB-Audio website.';
		}
	}

	onMount(() => {
		void loadStatus();
		const onFocus = () => void loadStatus(false);
		window.addEventListener('focus', onFocus);
		return () => window.removeEventListener('focus', onFocus);
	});
</script>

<section class="flex max-w-3xl flex-col gap-4 rounded-2xl bg-neutral-200 p-5">
	<div class="flex items-start gap-4">
		<div class="flex size-10 shrink-0 items-center justify-center rounded-xl bg-neutral-300">
			<Plug class={['size-5', ready ? 'text-emerald-600 dark:text-emerald-400' : needsRestart || incomplete ? 'text-amber-600' : 'text-neutral-800']} />
		</div>
		<div class="flex min-w-0 flex-1 flex-col gap-1">
			<h2 class="font-medium text-theme">VB-CABLE virtual microphone</h2>
			{#if checking}
				<p class="text-xs text-neutral-900">Checking Windows audio...</p>
			{:else if status?.state === 'installedExternal'}
				<p class="text-xs text-neutral-900">Installed outside Splitwave</p>
			{:else if status?.state === 'unknownOwnership'}
				<p class="text-xs text-neutral-900">Installed, ownership could not be verified</p>
			{:else if ready}
				<p class="text-xs text-emerald-700 dark:text-emerald-300">Ready</p>
			{:else if needsRestart}
				<p class="text-xs text-amber-700 dark:text-amber-300">Restart Windows to finish setup</p>
			{:else if incomplete}
				<p class="text-xs text-amber-700 dark:text-amber-300">Installation incomplete</p>
			{:else if status?.state === 'notInstalled'}
				<p class="text-xs text-neutral-900">Not installed</p>
			{:else}
				<p class="text-xs text-neutral-900">Status unavailable</p>
			{/if}
		</div>
		{#if !checking && status?.state === 'notInstalled'}
			<button type="button" class="button-main green rounded-lg" onclick={install} disabled={installing}>
				{installing ? 'Installing...' : 'Install virtual microphone'}
			</button>
		{:else if !installing && (issue || needsRestart || incomplete || status?.state === 'unknownOwnership')}
			<button type="button" class="button-main primary rounded-lg" onclick={() => loadStatus(true)}>Check again</button>
		{/if}
	</div>

	{#if ready}
		<div class="grid gap-2 rounded-xl bg-neutral-100 p-3 sm:grid-cols-2">
			<div class="flex flex-col gap-0.5">
				<span class="text-[10px] font-semibold tracking-wide text-neutral-800 uppercase">Speaker output</span>
				<span class="font-mono text-xs text-theme">{status?.renderEndpointName}</span>
			</div>
			<div class="flex flex-col gap-0.5">
				<span class="text-[10px] font-semibold tracking-wide text-neutral-800 uppercase">Microphone input</span>
				<span class="font-mono text-xs text-theme">{status?.captureEndpointName}</span>
			</div>
		</div>
	{/if}

	{#if needsRestart}
		<p class="text-xs leading-relaxed text-neutral-1000">
			VB-CABLE is present, but Windows has not exposed both audio endpoints yet. Splitwave checks again when this window regains focus.
		</p>
	{:else if status?.state === 'installedExternal'}
		<p class="text-xs leading-relaxed text-neutral-1000">Splitwave will use this installation without claiming or removing it.</p>
	{:else if status?.state === 'unknownOwnership'}
		<p class="text-xs leading-relaxed text-neutral-1000">The existing driver will be preserved and not changed.</p>
	{:else if incomplete}
		<p class="text-xs leading-relaxed text-neutral-1000">Splitwave found only part of the expected VB-CABLE installation and will not modify it.</p>
	{:else if status?.state === 'notInstalled'}
		<p class="text-xs leading-relaxed text-neutral-1000">
			Splitwave downloads the standard VB-Audio package from its official source. Administrator approval and a Windows restart may be required.
		</p>
	{/if}

	{#if status?.detail}
		<p class="text-xs leading-relaxed text-neutral-900">{status.detail}</p>
	{/if}

	{#if issue}
		<p class="rounded-xl border border-red-500/30 bg-red-500/10 px-3 py-2.5 text-xs leading-relaxed text-red-700 dark:text-red-300">{issue}</p>
	{/if}

	<div class="flex items-center gap-3 border-t border-neutral-300 pt-3 text-xs text-neutral-900">
		<span>VB-CABLE is third-party donationware by VB-Audio.</span>
		<button type="button" class="text-blue-600 underline-offset-2 hover:underline" onclick={learnAboutVbCable}>About VB-CABLE</button>
	</div>
</section>
