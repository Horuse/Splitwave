import { invoke } from '@tauri-apps/api/core';

export const methods = {
	isAvailable: (): Promise<boolean> => invoke<boolean>('tray_available'),
	setCloseToTray: (enabled: boolean): Promise<void> => invoke('set_close_to_tray', { enabled }),
	showMainWindow: (): Promise<void> => invoke('show_main_window'),
	quitApp: (): Promise<void> => invoke('quit_app')
};
