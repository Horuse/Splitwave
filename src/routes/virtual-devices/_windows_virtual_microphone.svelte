<script lang="ts">
	import { openUrl } from '@tauri-apps/plugin-opener';
	import { onMount } from 'svelte';
	import { Plug } from '$lib/components/icons';
	import { methods } from '$lib/modules/audio/methods';
	import type { WindowsVirtualCableStatus } from '$lib/modules/audio/types';
	import { t } from '$lib/modules/i18n';

	let status = $state<WindowsVirtualCableStatus | null>(null);
	let checking = $state(true);
	let installing = $state(false);
	// Kept as the raw error plus a fallback key, so the message re-translates
	// when the language changes instead of freezing the language it was shown in.
	let issue = $state<{ value: unknown; fallbackKey: string } | null>(null);
	let issueText = $derived(issue ? (friendlyIssue(issue.value) ?? t(issue.fallbackKey)) : null);

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

	function friendlyIssue(value: unknown): string | null {
		switch (errorCode(value)) {
			case 'elevationCancelled':
				return t('vdev.windows.issue.elevationCancelled');
			case 'downloadFailed':
				return t('vdev.windows.issue.downloadFailed');
			case 'downloadTimedOut':
				return t('vdev.windows.issue.downloadTimedOut');
			case 'checksumMismatch':
				return t('vdev.windows.issue.checksumMismatch');
			case 'invalidSignature':
				return errorMessage(value) ?? t('vdev.windows.issue.invalidSignature');
			case 'driverPackageNotDetected':
				return t('vdev.windows.issue.driverPackageNotDetected');
			case 'helperResultMissing':
			case 'helperResultInvalid':
				return t('vdev.windows.issue.helperResultMissing');
			case 'operationInProgress':
				return t('vdev.windows.issue.operationInProgress');
			case 'installerTimedOut':
			case 'helperTimedOut':
				return t('vdev.windows.issue.installerTimedOut');
			default:
				// The backend's own message names the failing step; it is shown
				// verbatim rather than flattened into a generic string.
				return errorMessage(value);
		}
	}

	async function loadStatus(showLoading = status === null) {
		if (installing) return;
		checking = showLoading;
		try {
			status = await methods.windowsVirtualCableStatus();
			issue = null;
		} catch (error) {
			issue = { value: error, fallbackKey: 'vdev.windows.issue.statusCheckFailed' };
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
			issue = { value: error, fallbackKey: 'vdev.windows.issue.installFailed' };
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
			await openUrl('https://vb-cable.com/');
		} catch {
			issue = { value: null, fallbackKey: 'vdev.windows.issue.openWebsiteFailed' };
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
			<h2 class="font-medium text-theme">{t('vdev.windows.title')}</h2>
			{#if checking}
				<p class="text-xs text-neutral-900">{t('vdev.windows.checking')}</p>
			{:else if status?.state === 'installedExternal'}
				<p class="text-xs text-neutral-900">{t('vdev.windows.installedExternal')}</p>
			{:else if status?.state === 'unknownOwnership'}
				<p class="text-xs text-neutral-900">{t('vdev.windows.unknownOwnership')}</p>
			{:else if ready}
				<p class="text-xs text-emerald-700 dark:text-emerald-300">{t('vdev.windows.ready')}</p>
			{:else if needsRestart}
				<p class="text-xs text-amber-700 dark:text-amber-300">{t('vdev.windows.restartRequired')}</p>
			{:else if incomplete}
				<p class="text-xs text-amber-700 dark:text-amber-300">{t('vdev.windows.incomplete')}</p>
			{:else if status?.state === 'notInstalled'}
				<p class="text-xs text-neutral-900">{t('vdev.windows.notInstalled')}</p>
			{:else}
				<p class="text-xs text-neutral-900">{t('vdev.windows.statusUnavailable')}</p>
			{/if}
		</div>
		{#if !checking && status?.state === 'notInstalled'}
			<button type="button" class="button-main green rounded-lg" onclick={install} disabled={installing}>
				{installing ? t('common.installing') : t('vdev.windows.installVirtualMic')}
			</button>
		{:else if !installing && (issue || needsRestart || incomplete || status?.state === 'unknownOwnership')}
			<button type="button" class="button-main primary rounded-lg" onclick={() => loadStatus(true)}>{t('vdev.windows.checkAgain')}</button>
		{/if}
	</div>

	{#if ready}
		<div class="grid gap-2 rounded-xl bg-neutral-100 p-3 sm:grid-cols-2">
			<div class="flex flex-col gap-0.5">
				<span class="text-[10px] font-semibold tracking-wide text-neutral-800 uppercase">{t('vdev.windows.speakerOutput')}</span>
				<span class="font-mono text-xs text-theme">{status?.renderEndpointName}</span>
			</div>
			<div class="flex flex-col gap-0.5">
				<span class="text-[10px] font-semibold tracking-wide text-neutral-800 uppercase">{t('vdev.windows.microphoneInput')}</span>
				<span class="font-mono text-xs text-theme">{status?.captureEndpointName}</span>
			</div>
		</div>
	{/if}

	{#if needsRestart}
		<p class="text-xs leading-relaxed text-neutral-1000">
			{t('vdev.windows.restartDetail')}
		</p>
	{:else if status?.state === 'installedExternal'}
		<p class="text-xs leading-relaxed text-neutral-1000">{t('vdev.windows.externalDetail')}</p>
	{:else if status?.state === 'unknownOwnership'}
		<p class="text-xs leading-relaxed text-neutral-1000">{t('vdev.windows.unknownDetail')}</p>
	{:else if incomplete}
		<p class="text-xs leading-relaxed text-neutral-1000">{t('vdev.windows.incompleteDetail')}</p>
	{:else if status?.state === 'notInstalled'}
		<p class="text-xs leading-relaxed text-neutral-1000">
			{t('vdev.windows.installDetail')}
		</p>
	{/if}

	{#if status?.detail}
		<p class="text-xs leading-relaxed text-neutral-900">{status.detail}</p>
	{/if}

	{#if issueText}
		<p class="rounded-xl border border-red-500/30 bg-red-500/10 px-3 py-2.5 text-xs leading-relaxed text-red-700 dark:text-red-300">{issueText}</p>
	{/if}

	<div class="border-t border-neutral-300 pt-3 text-xs leading-relaxed text-neutral-900">
		{t('vdev.windows.origin')}
		<button type="button" class="text-blue-600 underline-offset-2 hover:underline" onclick={learnAboutVbCable}>www.vb-cable.com</button>{t(
			'vdev.windows.donationware'
		)}
	</div>
</section>
