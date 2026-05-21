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

	onMount(() => {
		installErrorHandlers().catch(() => {});
		loadAppInfo().catch(() => {});
		audioStore.init().catch(() => {});
		pipelineStore.refresh().catch(() => {});
		checkForUpdates(true).catch(() => {});
		listen<string>('menu://action', (e) => handleMenu(e.payload))
			.then((fn) => { unlistenMenu = fn; })
			.catch(() => {});
	});

	onDestroy(() => {
		unlistenMenu?.();
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

