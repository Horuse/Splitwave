<script lang="ts">
	import type { Snippet } from 'svelte';
	import { themeStore } from '$lib/modules/theme/stores';
	import { platform } from '@tauri-apps/plugin-os';
	import { getCurrentWindow } from '@tauri-apps/api/window';
	import { modalManager } from '$lib/modules/overlay/modal';
	import { AboutModal } from '$lib/modules/about/ui';
	import { checkForUpdates } from '$lib/modules/updater';
	import { pipelineStore } from '$lib/modules/pipeline/stores.svelte';
	import { Popover } from '$lib/modules/overlay/ui';

	interface Props {
		left?: Snippet;
		right?: Snippet;
	}

	let { left, right }: Props = $props();

	// macOS keeps its native menu + overlay traffic lights. On Linux the window
	// is decoration-less, so the header carries the window controls and the
	// menu actions that used to live in the native menu bar.
	const isLinux = platform() === 'linux';
	const win = isLinux ? getCurrentWindow() : null;

	let menuOpen = $state(false);

	function about() {
		menuOpen = false;
		modalManager.open('', AboutModal, { canClose: true });
	}
	function checkUpdates() {
		menuOpen = false;
		checkForUpdates().catch(() => {});
	}
	function undo() {
		menuOpen = false;
		pipelineStore.editorActions?.undo();
	}
	function redo() {
		menuOpen = false;
		pipelineStore.editorActions?.redo();
	}
</script>

<header
	data-tauri-drag-region
	class={[
		'flex h-10 z-500 w-full flex-row gap-8 items-center border-b border-theme/5 bg-background px-1.5',
		isLinux ? 'pl-3' : 'pl-20'
	]}
>
	{@render left?.()}

	<div class="ml-auto flex items-center flex-row gap-4">
		{@render right?.()}

		<button
			class="button-header size-7"
			aria-label="Theme toggle"
			onclick={() => ($themeStore == 'light' ? ($themeStore = 'dark') : ($themeStore = 'light'))}
		>
			<svg class="size-4 fill-current" fill="none" viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"
				><path d="M12 22c5.523 0 10-4.477 10-10S17.523 2 12 2 2 6.477 2 12s4.477 10 10 10Zm0-2V4a8 8 0 1 1 0 16Z" /></svg
			>
		</button>

		{#if isLinux}
			<Popover bind:open={menuOpen} placement="bottom-end" offsetPx={6}>
				{#snippet trigger()}
					<button type="button" class="button-header size-7" aria-label="Menu">
						<svg class="size-4" fill="none" stroke="currentColor" stroke-width="1.6" viewBox="0 0 24 24"
							><path stroke-linecap="round" d="M4 7h16M4 12h16M4 17h16" /></svg
						>
					</button>
				{/snippet}

				<div class="min-w-48 overflow-hidden rounded-lg border border-neutral-400 bg-neutral-100 py-1 shadow-lg">
					<button type="button" class="block w-full px-3 py-1.5 text-left text-sm hover:bg-neutral-200" onclick={about}>About Splitwave</button>
					<button type="button" class="block w-full px-3 py-1.5 text-left text-sm hover:bg-neutral-200" onclick={checkUpdates}>Check for Updates…</button>
					<div class="my-1 border-t border-neutral-300"></div>
					<button type="button" class="block w-full px-3 py-1.5 text-left text-sm hover:bg-neutral-200" onclick={undo}>Undo</button>
					<button type="button" class="block w-full px-3 py-1.5 text-left text-sm hover:bg-neutral-200" onclick={redo}>Redo</button>
				</div>
			</Popover>

			<div class="flex items-center gap-0.5">
				<button class="button-header size-7" aria-label="Minimize" onclick={() => win?.minimize()}>
					<svg class="size-4" fill="none" stroke="currentColor" stroke-width="1.6" viewBox="0 0 24 24"
						><path stroke-linecap="round" d="M5 12h14" /></svg
					>
				</button>
				<button class="button-header size-7" aria-label="Maximize" onclick={() => win?.toggleMaximize()}>
					<svg class="size-3.5" fill="none" stroke="currentColor" stroke-width="1.6" viewBox="0 0 24 24"
						><rect x="5" y="5" width="14" height="14" rx="2" /></svg
					>
				</button>
				<button class="button-header size-7 hover:!bg-red-500 hover:!text-white" aria-label="Close" onclick={() => win?.close()}>
					<svg class="size-4" fill="none" stroke="currentColor" stroke-width="1.6" viewBox="0 0 24 24"
						><path stroke-linecap="round" d="M6 6l12 12M18 6 6 18" /></svg
					>
				</button>
			</div>
		{/if}
	</div>
</header>
