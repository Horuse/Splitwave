import { invoke } from '@tauri-apps/api/core';
import type { LogLine } from './types';

export const methods = {
	getLogs: (): Promise<LogLine[]> => invoke<LogLine[]>('get_logs'),
	clearLogs: (): Promise<void> => invoke('clear_logs')
};
