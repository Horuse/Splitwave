import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { errorStore } from './stores.svelte';

const PANIC_EVENT = 'error://panic';

interface CrashPayload {
	kind?: 'rustPanic' | 'nativeCrash' | 'unexpectedExit' | 'hang';
	message: string;
	backtrace: string;
	thread: string;
	version: string;
	ts?: number;
	logs?: string;
}

let installed = false;
let unlistenPanic: UnlistenFn | undefined;

function logToBackend(message: string, stack?: string): void {
	invoke('log_frontend', { message: stack ? `${message}\n${stack}` : message }).catch(() => {});
}

export async function installErrorHandlers(): Promise<void> {
	if (installed) return;
	installed = true;

	unlistenPanic = await listen<CrashPayload>(PANIC_EVENT, (e) => {
		errorStore.report({
			source: 'rustPanic',
			message: e.payload.message,
			stack: e.payload.backtrace,
			thread: e.payload.thread,
			at: Date.now()
		});
	});

	// Fatal native failures cannot reach the live webview; replay every report
	// persisted by the backend during the previous run.
	try {
		const reports = await invoke<CrashPayload[]>('take_crash_reports');
		for (const r of reports) {
			errorStore.report({
				source: r.kind ?? 'rustPanic',
				message: r.message,
				stack: r.backtrace,
				thread: r.thread,
				at: r.ts ?? Date.now(),
				logs: r.logs,
				previousRun: true
			});
		}
	} catch {}

	window.addEventListener('error', (e) => {
		if (e.message?.startsWith('ResizeObserver loop')) return;
		const message = e.message || String(e.error);
		const stack = e.error instanceof Error ? e.error.stack : undefined;
		logToBackend(message, stack);
		errorStore.report({ source: 'jsError', message, stack, at: Date.now() });
	});

	window.addEventListener('unhandledrejection', (e) => {
		const reason = e.reason;
		const message = reason instanceof Error ? reason.message : typeof reason === 'string' ? reason : JSON.stringify(reason);
		const stack = reason instanceof Error ? reason.stack : undefined;
		logToBackend(`unhandled rejection: ${message}`, stack);
		errorStore.report({ source: 'unhandledRejection', message, stack, at: Date.now() });
	});
}

export function uninstallErrorHandlers(): void {
	unlistenPanic?.();
	unlistenPanic = undefined;
	installed = false;
}
