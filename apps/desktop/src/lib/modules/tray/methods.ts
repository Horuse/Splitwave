import { invoke } from '@tauri-apps/api/core';
import type { TrayStatus } from './types';

export const methods = {
	/** Picks the tray icon: plain, badged while running, or warning while something is missing. */
	setStatus: (status: TrayStatus): Promise<void> => invoke('set_tray_status', { status }),
	isAvailable: (): Promise<boolean> => invoke<boolean>('tray_available'),
	setCloseToTray: (enabled: boolean): Promise<void> => invoke('set_close_to_tray', { enabled }),
	showMainWindow: (): Promise<void> => invoke('show_main_window'),
	quitApp: (): Promise<void> => invoke('quit_app')
};
