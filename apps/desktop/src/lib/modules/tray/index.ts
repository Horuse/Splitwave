import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

const TOGGLE_EVENT = 'tray://toggle_pipeline';

export interface TrayPipeline {
	id: string;
	name: string;
}

/** Rebuilds the macOS menu-bar pipeline list. No-op on other platforms. */
export function syncTrayMenu(pipelines: TrayPipeline[], runningId: string | null): Promise<void> {
	return invoke('sync_tray_menu', { pipelines, runningId });
}

export function onTrayTogglePipeline(cb: (pipelineId: string) => void): Promise<UnlistenFn> {
	return listen<string>(TOGGLE_EVENT, (e) => cb(e.payload));
}

/** Brings the main window back when a menu-bar action fails while it is hidden. */
export function showMainWindow(): Promise<void> {
	return invoke('show_main_window');
}
