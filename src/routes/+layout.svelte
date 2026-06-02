<script lang="ts">
	import '../app.css';
	import { onDestroy, onMount } from 'svelte';
	import { listen, type UnlistenFn } from '@tauri-apps/api/event';
	import { Toaster } from 'svelte-french-toast';
	import { audioStore } from '$lib/modules/audio/stores.svelte';
	import { pipelineStore } from '$lib/modules/pipeline/stores.svelte';
	import { installErrorHandlers } from '$lib/modules/error';
	import { ErrorModal } from '$lib/modules/error/ui';
	import { checkForUpdates } from '$lib/modules/updater';
	import { UpdateBanner } from '$lib/modules/updater/ui';
	import { DebugPanel } from '$lib/modules/debug';
	import { loadAppInfo } from '$lib/modules/app_info';
	import { ModalRender } from '$lib/modules/overlay/ui';
	import { modalManager } from '$lib/modules/overlay/modal';
	import { AboutModal } from '$lib/modules/about/ui';
	import { platform } from '@tauri-apps/plugin-os';

	const isDev = import.meta.env.DEV;

	let { children } = $props();

	let unlistenMenu: UnlistenFn | undefined;

	function handleMenu(id: string) {
		switch (id) {
			case 'about':
				modalManager.open('', AboutModal, { canClose: true });
				break;
			case 'check_updates':
				checkForUpdates().catch(() => {});
				break;
			case 'undo':
				pipelineStore.editorActions?.undo();
				break;
			case 'redo':
				pipelineStore.editorActions?.redo();
				break;
		}
	}

	// Linux has no native menu, so its Cmd/Ctrl+Z accelerators are gone -- wire
	// undo/redo here. Skip while typing so text-field undo still works.
	function onKeydown(e: KeyboardEvent) {
		if (!(e.ctrlKey || e.metaKey) || e.key.toLowerCase() !== 'z') return;
		const t = e.target as HTMLElement | null;
		if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable)) return;
		e.preventDefault();
		if (e.shiftKey) pipelineStore.editorActions?.redo();
		else pipelineStore.editorActions?.undo();
	}

	onMount(() => {
		installErrorHandlers().catch(() => {});
		loadAppInfo().catch(() => {});
		audioStore.init().catch(() => {});
		pipelineStore.refresh().catch(() => {});
		checkForUpdates(true).catch(() => {});
		listen<string>('menu://action', (e) => handleMenu(e.payload))
			.then((fn) => { unlistenMenu = fn; })
			.catch(() => {});
		if (platform() === 'linux') window.addEventListener('keydown', onKeydown);
	});

	onDestroy(() => {
		unlistenMenu?.();
		window.removeEventListener('keydown', onKeydown);
		audioStore.destroy();
	});
</script>

<UpdateBanner />

<main>
	{@render children()}
</main>

<ErrorModal />

<ModalRender />

{#if isDev}
	<DebugPanel />
{/if}

