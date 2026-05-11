<script lang="ts">
	import '../app.css';
	import { onDestroy, onMount } from 'svelte';
	import type { UnlistenFn } from '@tauri-apps/api/event';
	import { AppStore, provideAppStore } from '$lib/stores/app-store.svelte';
	import { PipelineRepository } from '$lib/services/pipeline-repository';
	import { TauriAudioService } from '$lib/services/audio-service';
    import {themeStore} from "$lib/stores/theme";

	let { children } = $props();

	const store = new AppStore(new PipelineRepository(), new TauriAudioService());
	provideAppStore(store);

	let unlisten: UnlistenFn | undefined;

	onMount(async () => {
		await Promise.all([store.refreshPipelines(), store.refreshDevices()]);
		unlisten = await store.audio.onState((e) => {
			if (e.kind === 'started') {
				store.isRunning = true;
				store.lastError = null;
			} else if (e.kind === 'stopped') {
				store.isRunning = false;
			} else if (e.kind === 'error') {
				store.isRunning = false;
				store.lastError = e.message;
			}
		});
	});

	onDestroy(() => unlisten?.());

    // import { onNavigate } from '$app/navigation'
    //
    // // Transition API
    // onNavigate((navigation) => {
    //     if (!document.startViewTransition) return
    //
    //     return new Promise((resolve) => {
    //         document.startViewTransition(async () => {
    //             resolve()
    //             await navigation.complete
    //         })
    //     })
    // })

    // if (browser) {
    //     if (window.matchMedia('(prefers-color-scheme: dark)').matches) {
    //         document.documentElement.classList.add('dark')
    //     } else {
    //         document.documentElement.classList.remove('dark')
    //     }
    // }

    import Header from '$lib/components/layout/header.svelte';
    import { setContext } from 'svelte';

    let header;

    setContext('header', header);
</script>

<main>
    {@render children()}
</main>
