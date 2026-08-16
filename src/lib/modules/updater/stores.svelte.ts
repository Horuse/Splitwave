import type { Update } from '@tauri-apps/plugin-updater';

export type UpdateState =
	| { phase: 'idle' }
	| { phase: 'checking' }
	| { phase: 'up_to_date' }
	| { phase: 'available'; update: Update; notes: string | null }
	| { phase: 'downloading'; update: Update; downloaded: number; total: number | null }
	| { phase: 'installing'; update: Update }
	| { phase: 'unsupported'; message: string }
	| { phase: 'error'; message: string };

class UpdaterStore {
	state = $state<UpdateState>({ phase: 'idle' });
}

export const updaterStore = new UpdaterStore();
