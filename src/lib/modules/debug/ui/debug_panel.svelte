<script lang="ts">
	import { fly } from 'svelte/transition';
	import type { Update } from '@tauri-apps/plugin-updater';
	import { errorStore } from '$lib/modules/error';
	import { updaterStore } from '$lib/modules/updater';
	import { Menu, MenuItem, MenuSection, MenuSeparator } from '$lib/modules/overlay/ui';

	let open = $state(false);

	function fakeRustPanic() {
		errorStore.report({
			source: 'rustPanic',
			message: "panicked at 'index out of bounds: the len is 0 but the index is 0'",
			stack:
				'   0: std::backtrace_rs::backtrace::libunwind::trace\n   1: core::panicking::panic_fmt\n   2: splitwave_lib::audio::pipeline::worker::run\n   3: std::sys_common::backtrace::__rust_begin_short_backtrace',
			thread: 'dsp-worker',
			at: Date.now()
		});
	}

	function fakeJsError() {
		errorStore.report({
			source: 'jsError',
			message: "Cannot read properties of undefined (reading 'foo')",
			stack:
				"TypeError: Cannot read properties of undefined (reading 'foo')\n    at editor.svelte:42:10",
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

	function fakeUpdateAvailable() {
		const stub = {
			version: '0.2.0',
			currentVersion: '0.1.0',
			date: new Date().toISOString(),
			body: 'Bug fixes and performance improvements.',
			downloadAndInstall: async () => {},
			download: async () => {},
			install: async () => {},
			close: async () => {}
		} as unknown as Update;
		updaterStore.state = { phase: 'available', update: stub };
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

	function clearAll() {
		errorStore.dismiss();
		updaterStore.state = { phase: 'idle' };
	}
</script>

<div class="fixed right-3 bottom-3 z-[200] flex flex-col items-end gap-1">
	{#if open}
		<div transition:fly={{ duration: 200, y: 5 }}>
			<Menu>
				<MenuSection label="Errors" />
				<MenuItem label="Rust panic" onclick={fakeRustPanic} />
				<MenuItem label="JS error" onclick={fakeJsError} />
				<MenuItem label="Promise rejection" onclick={fakePromiseRejection} />
				<MenuSection label="Updater" />
				<MenuItem label="Update available" onclick={fakeUpdateAvailable} />
				<MenuItem label="Downloading 30%" onclick={fakeDownloading} />
				<MenuItem label="Update error" onclick={fakeUpdateError} />
				<MenuSeparator />
				<MenuItem label="Clear all" onclick={clearAll} />
			</Menu>
		</div>
	{/if}

	<button
		type="button"
		class="button-header size-7 px-3 text-xs"
		onclick={() => (open = !open)}
		title="Dev triggers"
	>
		{open ? 'Close' : 'Dev'}
	</button>
</div>
