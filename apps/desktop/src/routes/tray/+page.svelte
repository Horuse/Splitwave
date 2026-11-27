<script lang="ts">
	import { onMount } from 'svelte';
	import { emitTo, listen } from '@tauri-apps/api/event';
	import { getCurrentWindow } from '@tauri-apps/api/window';
	import { TrayPanel } from '$lib/modules/tray/ui';
	import { methods } from '$lib/modules/tray/methods';
	import { TRAY_INTENT_EVENT, TRAY_STATE_EVENT, type TrayIntent, type TraySnapshot } from '$lib/modules/tray/types';
	import { themeStore } from '$lib/modules/theme/stores';

	let snapshot = $state<TraySnapshot | null>(null);

	function send(intent: TrayIntent) {
		emitTo('main', TRAY_INTENT_EVENT, intent).catch(() => {});
	}

	function onKeydown(e: KeyboardEvent) {
		if (e.key === 'Escape') getCurrentWindow().hide();
	}

	onMount(() => {
		// The window is transparent so the panel's rounded corners show.
		document.body.classList.add('!bg-transparent');
		const stateListener = listen<TraySnapshot>(TRAY_STATE_EVENT, ({ payload }) => {
			snapshot = payload;
			$themeStore = payload.theme;
		});
		// The panel stays mounted while hidden, so each opening asks for fresh state.
		const focusListener = getCurrentWindow().onFocusChanged(({ payload: focused }) => {
			if (focused) send({ kind: 'sync' });
		});
		stateListener.then(() => send({ kind: 'sync' }));
		return () => {
			stateListener.then((un) => un());
			focusListener.then((un) => un());
		};
	});
</script>

<svelte:window onkeydown={onKeydown} oncontextmenu={(e) => e.preventDefault()} />

<div class="h-screen overflow-hidden rounded-xl">
	{#if snapshot}
		<TrayPanel
			pipelines={snapshot.pipelines}
			runningId={snapshot.runningId}
			isRunning={snapshot.isRunning}
			busyId={snapshot.busyId}
			error={snapshot.error}
			onrun={(pipelineId) => send({ kind: 'run', pipelineId })}
			onstop={() => send({ kind: 'stop' })}
			onopen={() => methods.showMainWindow()}
			onquit={() => methods.quitApp()} />
	{/if}
</div>
