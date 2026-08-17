<script lang="ts">
	import { openUrl } from '@tauri-apps/plugin-opener';
	import { onMount } from 'svelte';
	import { ArrowRight, Mic, Plug, SoundWave } from '$lib/components/icons';
	import { modalManager, type ModalBaseProps } from '$lib/modules/overlay/modal';
	import { methods } from '../methods';
	import type { WindowsVirtualCableStatus } from '../types';

	interface Props {
		initialStatus: WindowsVirtualCableStatus | null;
	}

	let { modalId, initialStatus }: ModalBaseProps & Props = $props();

	let status = $state<WindowsVirtualCableStatus | null>(null);
	let installing = $state(false);
	let checking = $state(true);
	let issue = $state<string | null>(null);

	const needsRestart = $derived(status?.state === 'partial' || status?.state === 'rebootRequired' || status?.state === 'removalPendingReboot');
	const cannotManage = $derived(status?.state === 'unknownOwnership');

	function errorCode(value: unknown): string | null {
		if (!value || typeof value !== 'object' || !('code' in value)) return null;
		return typeof value.code === 'string' ? value.code : null;
	}

	function friendlyIssue(value: unknown): string {
		switch (errorCode(value)) {
			case 'elevationCancelled':
				return 'Administrator approval was cancelled. Nothing else is needed unless you want to try again.';
			case 'downloadFailed':
				return 'Splitwave could not download VB-CABLE. Check your connection and try again.';
			case 'checksumMismatch':
			case 'invalidSignature':
				return 'The downloaded driver could not be verified, so Splitwave did not run it.';
			case 'operationInProgress':
				return 'Another virtual microphone operation is already in progress.';
			default:
				return 'The virtual microphone could not be installed. Splitwave preserved the current Windows audio configuration.';
		}
	}

	async function loadStatus() {
		checking = true;
		try {
			status = await methods.windowsVirtualCableStatus();
			issue = null;
		} catch {
			issue = 'Splitwave could not check the Windows virtual microphone status.';
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
			} catch {
				// Keep the actionable installation message when the follow-up probe also fails.
			}
		} finally {
			installing = false;
		}

		if (status?.usable && status.renderEndpointName) modalManager.close(modalId, status);
	}

	async function learnAboutVbCable() {
		try {
			await openUrl('https://vb-audio.com/Cable/');
		} catch {
			issue = 'Splitwave could not open the VB-Audio website.';
		}
	}

	function close() {
		modalManager.close(modalId);
	}

	onMount(() => {
		status = initialStatus;
		checking = initialStatus === null;
		if (initialStatus === null) void loadStatus();
	});
</script>

<div class="flex flex-col px-5 pt-4 pb-5">
	{#if checking}
		<div class="flex min-h-44 items-center justify-center text-sm text-neutral-900">Checking Windows audio…</div>
	{:else if needsRestart}
		<div class="flex flex-col gap-4">
			<div class="flex items-start gap-3 rounded-xl border border-amber-500/30 bg-amber-500/10 p-3.5">
				<div class="flex size-9 shrink-0 items-center justify-center rounded-lg bg-amber-500/15 text-amber-700 dark:text-amber-300">
					<Plug class="size-4.5" />
				</div>
				<div class="flex flex-col gap-1">
					<h3 class="text-sm font-semibold text-theme">Restart Windows to finish</h3>
					<p class="text-xs leading-relaxed text-neutral-1000">
						VB-CABLE is present, but Windows has not exposed both audio endpoints yet. Splitwave will detect them automatically after restart.
					</p>
				</div>
			</div>
			{#if status?.captureEndpointName || status?.renderEndpointName}
				<p class="text-xs text-neutral-900">
					Detected so far: {status.renderEndpointName ?? status.captureEndpointName}
				</p>
			{/if}
			<div class="flex justify-end">
				<button type="button" class="button-main primary rounded-lg" onclick={close}>Close</button>
			</div>
		</div>
	{:else if cannotManage}
		<div class="flex flex-col gap-4">
			<div class="flex items-start gap-3 rounded-xl border border-neutral-400 bg-neutral-200 p-3.5">
				<Plug class="mt-0.5 size-4.5 shrink-0 text-neutral-1000" />
				<div class="flex flex-col gap-1">
					<h3 class="text-sm font-semibold text-theme">VB-CABLE needs attention</h3>
					<p class="text-xs leading-relaxed text-neutral-1000">
						Splitwave found VB-CABLE but cannot safely verify its ownership. The driver will be preserved and not changed.
					</p>
				</div>
			</div>
			<div class="flex justify-end">
				<button type="button" class="button-main primary rounded-lg" onclick={close}>Close</button>
			</div>
		</div>
	{:else}
		<div class="flex flex-col gap-5">
			<p class="text-sm leading-relaxed text-neutral-1100">Splitwave uses VB-CABLE to make your mix available as a microphone in other apps.</p>

			<div class="grid grid-cols-[1fr_auto_1fr_auto_1fr_auto_1fr] items-start gap-2 text-center">
				<div class="flex flex-col items-center gap-1.5">
					<div class="flex size-9 items-center justify-center rounded-full border border-neutral-400 bg-neutral-100 text-blue-600">
						<SoundWave class="size-4" />
					</div>
					<span class="text-[10px] text-neutral-1000">Splitwave</span>
				</div>
				<ArrowRight class="mt-3 size-3.5 text-neutral-800" />
				<div class="flex flex-col items-center gap-1.5">
					<div class="flex size-9 items-center justify-center rounded-full border border-neutral-400 bg-neutral-100">
						<Plug class="size-4" />
					</div>
					<span class="text-[10px] text-neutral-1000">CABLE Input</span>
				</div>
				<ArrowRight class="mt-3 size-3.5 text-neutral-800" />
				<div class="flex flex-col items-center gap-1.5">
					<div class="flex size-9 items-center justify-center rounded-full border border-neutral-400 bg-neutral-100">
						<Plug class="size-4" />
					</div>
					<span class="text-[10px] text-neutral-1000">CABLE Output</span>
				</div>
				<ArrowRight class="mt-3 size-3.5 text-neutral-800" />
				<div class="flex flex-col items-center gap-1.5">
					<div class="flex size-9 items-center justify-center rounded-full border border-neutral-400 bg-neutral-100">
						<Mic class="size-4" />
					</div>
					<span class="text-[10px] text-neutral-1000">Other apps</span>
				</div>
			</div>

			<div class="flex flex-col gap-1 text-xs leading-relaxed text-neutral-900">
				<p>VB-CABLE is third-party donationware by VB-Audio.</p>
				<p>Windows administrator approval and a restart may be required.</p>
				<button type="button" class="w-fit text-blue-600 underline-offset-2 hover:underline" onclick={learnAboutVbCable}>About VB-CABLE</button>
			</div>

			{#if issue}
				<p class="rounded-xl border border-amber-500/30 bg-amber-500/10 px-3 py-2.5 text-xs leading-relaxed text-amber-800 dark:text-amber-200">
					{issue}
				</p>
			{/if}

			<div class="flex justify-end gap-2 border-t border-neutral-300 pt-4">
				<button type="button" class="button-main primary rounded-lg" onclick={close} disabled={installing}>Not now</button>
				<button
					type="button"
					class="button-main green rounded-lg"
					onclick={status === null ? loadStatus : install}
					disabled={installing || (status !== null && status.state !== 'notInstalled')}>
					{installing ? 'Installing…' : status === null ? 'Check again' : issue ? 'Try again' : 'Install & continue'}
				</button>
			</div>
		</div>
	{/if}
</div>
