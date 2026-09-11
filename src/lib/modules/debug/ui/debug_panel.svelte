<script lang="ts">
	import { fly } from 'svelte/transition';
	import { invoke } from '@tauri-apps/api/core';
	import type { Update } from '@tauri-apps/plugin-updater';
	import { errorStore } from '$lib/modules/error';
	import { audioStore } from '$lib/modules/audio/stores.svelte';
	import { updaterStore, latestRelease } from '$lib/modules/updater';
	import { getCachedAppInfo } from '$lib/modules/app_info';
	import { Menu, MenuItem, MenuSection, MenuSeparator } from '$lib/modules/overlay/ui';
	import { announcementStore, runModalQueue } from '$lib/modules/announcements';

	let open = $state(false);

	function fakeRustPanic() {
		errorStore.report({
			source: 'rustPanic',
			message: "panicked at 'index out of bounds: the len is 0 but the index is 0'",
			stack: '   0: std::backtrace_rs::backtrace::libunwind::trace\n   1: core::panicking::panic_fmt\n   2: splitwave_lib::audio::pipeline::worker::run\n   3: std::sys_common::backtrace::__rust_begin_short_backtrace',
			thread: 'dsp-worker',
			at: Date.now()
		});
	}

	// Real backend panic on the main thread: crashes the app to exercise crash
	// persistence + the next-launch modal. Not a faked event.
	function realRustCrash() {
		invoke('debug_panic').catch(() => {});
	}

	function nativeCrash() {
		invoke('debug_native_crash').catch(() => {});
	}

	function unexpectedExit() {
		invoke('debug_unexpected_exit').catch(() => {});
	}

	function fakeJsError() {
		errorStore.report({
			source: 'jsError',
			message: "Cannot read properties of undefined (reading 'foo')",
			stack: "TypeError: Cannot read properties of undefined (reading 'foo')\n    at editor.svelte:42:10",
			at: Date.now()
		});
	}

	function fakePromiseRejection() {
		errorStore.report({
			source: 'unhandledRejection',
			message: 'Tauri command "fetch_thing" failed: NotRunning',
			at: Date.now()
		});
	}

	async function fakeSafeMode() {
		if (audioStore.isRunning) {
			await audioStore.deactivatePipeline().catch(() => {});
		}
		audioStore.safeMode = true;
		errorStore.report({
			source: 'nativeCrash',
			message: 'Native crash: SIGSEGV (Process crashed during audio startup)',
			stack: 'The process terminated before a Rust backtrace could be captured. Use the OS crash dump for the native stack.',
			thread: '<native>',
			at: Date.now(),
			previousRun: true
		});
	}

	// Uses the real latest GitHub release so the modal shows notes of the shape
	// users actually get.
	async function fakeUpdateAvailable() {
		const release = await latestRelease();
		const stub = {
			version: release?.version ?? '0.0.0',
			currentVersion: getCachedAppInfo()?.appVersion ?? '0.0.0',
			date: new Date().toISOString(),
			downloadAndInstall: async () => {},
			download: async () => {},
			install: async () => {},
			close: async () => {}
		} as unknown as Update;
		updaterStore.state = {
			phase: 'available',
			update: stub,
			notes: release?.notes ?? 'Could not reach the GitHub releases API.'
		};
	}

	async function fakeBetaUpdateAvailable() {
		const release = await latestRelease();
		const stub = {
			version: '1.3.0-rc.1',
			currentVersion: getCachedAppInfo()?.appVersion ?? '1.2.0',
			date: new Date().toISOString(),
			downloadAndInstall: async () => {},
			download: async () => {},
			install: async () => {},
			close: async () => {}
		} as unknown as Update;
		updaterStore.state = {
			phase: 'available',
			update: stub,
			notes: release?.notes ?? '### Pre-release v1.3.0-rc.1\n\n- Safe mode on crash\n- Automatic pipeline backup\n- Pre-release beta channel'
		};
	}

	function fakeDownloading() {
		const stub = { version: '0.2.0' } as unknown as Update;
		updaterStore.state = {
			phase: 'downloading',
			update: stub,
			downloaded: 1_200_000,
			total: 4_000_000
		};
	}

	function fakeUpdateError() {
		updaterStore.state = { phase: 'error', message: 'signature verification failed' };
	}

	function fakeRcBanner() {
		announcementStore.bannerQueue = [
			{
				id: 'notice-testing-v130-rc',
				type: 'banner',
				severity: 'info',
				priority: 50,
				badge: 'RC TEST',
				title: 'Testing Splitwave 1.3.0 Release Candidate',
				message: 'You are running Splitwave 1.3.0-rc.1. Please report any audio or UI feedback on GitHub.',
				action: {
					label: 'Report Issue',
					url: 'https://github.com/Horuse/Splitwave/issues'
				},
				dismissible: true,
				filters: {
					channels: ['rc']
				}
			},
			...announcementStore.bannerQueue.filter((b) => b.id !== 'notice-testing-v130-rc')
		];
		announcementStore.currentBannerIndex = 0;
	}

	function fakeModalNotice() {
		announcementStore.modalQueue = [
			{
				id: 'notice-updater-resource-id-workaround',
				type: 'modal',
				severity: 'critical',
				priority: 100,
				badge: 'NOTICE',
				title: 'Manual Update Required for v1.1.0',
				message: 'In-app automatic updates are broken in v1.1.0. Please download the latest version manually once from splitwave.app or GitHub.',
				markdown:
					'### In-App Update Notice\n\nDue to an updater issue in version `1.1.0`, automatic updates fail with `The resource id is invalid`.\n\nTo update to the latest version, please download and install Splitwave manually once:\n- [Official Website](https://splitwave.app)\n- [GitHub Releases](https://github.com/Horuse/Splitwave/releases)\n\nYour existing pipelines, presets, and audio configuration will be preserved automatically.',
				action: {
					label: 'Download Latest',
					url: 'https://splitwave.app'
				},
				dismissible: true,
				filters: {
					versions: '1.1.0',
					channels: ['stable', 'rc', 'beta']
				}
			}
		];
		runModalQueue().catch(() => {});
	}

	function fakeQueueMultiple() {
		announcementStore.bannerQueue = [
			{
				id: 'banner-test-1',
				type: 'banner',
				severity: 'critical',
				priority: 100,
				badge: 'CRITICAL',
				title: 'Buffer Underrun Detected',
				message: 'Audio output buffer underflowed by 128 frames at 96kHz.',
				dismissible: true
			},
			{
				id: 'banner-test-2',
				type: 'banner',
				severity: 'info',
				priority: 50,
				badge: 'RC TEST',
				title: 'Testing Splitwave 1.3.0 Release Candidate',
				message: 'Please report any edge connection or VST3 editor issues on GitHub.',
				dismissible: true
			}
		];
		announcementStore.currentBannerIndex = 0;

		announcementStore.modalQueue = [
			{
				id: 'modal-test-1',
				type: 'modal',
				severity: 'warning',
				title: 'Audio Buffer Optimization Notice',
				message: 'Recommended buffer size was adjusted to minimize playback latency.',
				markdown:
					'### Notice #1\n\nThis tests the **sequential modal queue**. When you click Dismiss or Next, modal #2 opens automatically without overlapping popups.',
				dismissible: true
			},
			{
				id: 'modal-test-2',
				type: 'modal',
				severity: 'info',
				title: 'New Virtual Devices Available',
				message: 'Splitwave virtual audio loopback drivers are ready for testing.',
				markdown: '### Notice #2\n\nSequential queue verified! All modal announcements processed.',
				dismissible: true
			}
		];
		runModalQueue().catch(() => {});
	}

	function resetDismissedAnnouncements() {
		if (typeof window !== 'undefined') {
			window.localStorage.removeItem('announcements:dismissed');
		}
		announcementStore.dismissed = {};
		announcementStore.bannerQueue = [];
		announcementStore.modalQueue = [];
	}

	function clearAll() {
		errorStore.dismiss();
		updaterStore.state = { phase: 'idle' };
		announcementStore.bannerQueue = [];
		announcementStore.modalQueue = [];
	}
</script>

<div class="fixed right-3 bottom-3 z-[200] flex flex-col items-end gap-1">
	{#if open}
		<div transition:fly={{ duration: 200, y: 5 }}>
			<Menu>
				<MenuSection label="Errors" />
				<MenuItem label="Rust panic (preview)" onclick={fakeRustPanic} />
				<MenuItem label="Real crash (panic)" onclick={realRustCrash} />
				<MenuItem label="Native crash (process)" onclick={nativeCrash} />
				<MenuItem label="Unexpected exit (process)" onclick={unexpectedExit} />
				<MenuItem label="JS error" onclick={fakeJsError} />
				<MenuItem label="Promise rejection" onclick={fakePromiseRejection} />
				<MenuItem label="Safe Mode (simulate)" onclick={fakeSafeMode} />
				<MenuSection label="Updater" />
				<MenuItem label="Update available" onclick={fakeUpdateAvailable} />
				<MenuItem label="Pre-release update available" onclick={fakeBetaUpdateAvailable} />
				<MenuItem label="Downloading 30%" onclick={fakeDownloading} />
				<MenuItem label="Update error" onclick={fakeUpdateError} />
				<MenuSection label="Announcements" />
				<MenuItem label="Test RC Banner" onclick={fakeRcBanner} />
				<MenuItem label="Test Modal Notice" onclick={fakeModalNotice} />
				<MenuItem label="Test Queue (2 Banners + 2 Modals)" onclick={fakeQueueMultiple} />
				<MenuItem label="Reset Dismissed Notices" onclick={resetDismissedAnnouncements} />
				<MenuSeparator />
				<MenuItem label="Clear all" onclick={clearAll} />
			</Menu>
		</div>
	{/if}

	<button type="button" class="button-header size-7 px-3 text-xs" onclick={() => (open = !open)} title="Dev triggers">
		{open ? 'Close' : 'Dev'}
	</button>
</div>
