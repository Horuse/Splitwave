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
	import { appSettings } from '$lib/modules/settings/stores.svelte';
	import { UpdateBanner } from '$lib/modules/updater/ui';
	import { DebugPanel } from '$lib/modules/debug';
	import { loadAppInfo } from '$lib/modules/app_info';
	import { ModalRender } from '$lib/modules/overlay/ui';
	import { modalManager } from '$lib/modules/overlay/modal';
	import { AboutModal } from '$lib/modules/about/ui';
	import { logStore } from '$lib/modules/logs';
	import { LogsModal } from '$lib/modules/logs/ui';
	import { platform } from '@tauri-apps/plugin-os';

	const isDev = import.meta.env.DEV;

	let { children } = $props();

	let unlistenMenu: UnlistenFn | undefined;

	function focusedField(): HTMLInputElement | HTMLTextAreaElement | null {
		const el = document.activeElement;
		return el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement ? el : null;
	}

	// The macOS Edit menu routes Copy/Paste/Select All here instead of letting AppKit
	// handle them, so the text-field behaviour has to be reproduced by hand.
	function fieldCopy(el: HTMLInputElement | HTMLTextAreaElement) {
		const { selectionStart, selectionEnd } = el;
		if (selectionStart === null || selectionEnd === null || selectionStart === selectionEnd) return;
		navigator.clipboard.writeText(el.value.slice(selectionStart, selectionEnd)).catch(() => {});
	}

	async function fieldPaste(el: HTMLInputElement | HTMLTextAreaElement) {
		const text = await navigator.clipboard.readText();
		if (!text) return;
		const start = el.selectionStart ?? el.value.length;
		const end = el.selectionEnd ?? start;
		el.setRangeText(text, start, end, 'end');
		el.dispatchEvent(new Event('input', { bubbles: true }));
	}

	function handleMenu(id: string) {
		const field = focusedField();
		switch (id) {
			case 'copy':
				if (field) fieldCopy(field);
				else pipelineStore.editorActions?.copySelection();
				return;
			case 'paste':
				if (field) fieldPaste(field).catch(() => {});
				else pipelineStore.editorActions?.paste();
				return;
			case 'select_all':
				if (field) field.select();
				else pipelineStore.editorActions?.selectAll();
				return;
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

	// Linux and Windows have no native menu, so their Cmd/Ctrl+Z accelerators are
	// gone -- wire undo/redo here. Skip while typing so text-field undo still works.
	function onKeydown(e: KeyboardEvent) {
		if (!(e.ctrlKey || e.metaKey) || e.key.toLowerCase() !== 'z') return;
		const t = e.target as HTMLElement | null;
		if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable)) return;
		e.preventDefault();
		if (e.shiftKey) pipelineStore.editorActions?.redo();
		else pipelineStore.editorActions?.undo();
	}

	// Cmd/Ctrl+Shift+L opens the log viewer. Shipped in release builds too --
	// it is the only way to read engine logs on a user's machine.
	function onLogsHotkey(e: KeyboardEvent) {
		if (!(e.ctrlKey || e.metaKey) || !e.shiftKey || e.key.toLowerCase() !== 'l') return;
		e.preventDefault();
		logStore.open = !logStore.open;
	}

	onMount(() => {
		logStore.installConsoleCapture();
		window.addEventListener('keydown', onLogsHotkey);
		installErrorHandlers().catch(() => {});
		loadAppInfo().catch(() => {});
		audioStore.init().catch(() => {});
		pipelineStore.refresh().catch(() => {});
		if (appSettings.checkUpdatesOnLaunch) checkForUpdates(true).catch(() => {});
		listen<string>('menu://action', (e) => handleMenu(e.payload))
			.then((fn) => { unlistenMenu = fn; })
			.catch(() => {});
		// plugin-os reads a Tauri-injected global; absent in plain-browser preview.
		try {
			const os = platform();
			if (os === 'linux' || os === 'windows') window.addEventListener('keydown', onKeydown);
		} catch {
			// no OS plugin (preview) -- native menus/accelerators are irrelevant here
		}
	});

	onDestroy(() => {
		unlistenMenu?.();
		window.removeEventListener('keydown', onKeydown);
		window.removeEventListener('keydown', onLogsHotkey);
		audioStore.destroy();
	});
</script>

<UpdateBanner />

<main>
	{@render children()}
</main>

<ErrorModal />

{#if logStore.open}
	<LogsModal />
{/if}

<ModalRender />

{#if isDev}
	<DebugPanel />
{/if}

