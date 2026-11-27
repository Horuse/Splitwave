import type { ThemePref } from '$lib/modules/theme/stores';

/** Main window to panel. */
export const TRAY_STATE_EVENT = 'tray://state';
/** Panel to main window. */
export const TRAY_INTENT_EVENT = 'tray://intent';

export type TrayPipeline = {
	id: string;
	name: string;
	outputs: string[];
	/** Saved by a newer version, cannot be run. */
	stale: boolean;
};

export type TraySnapshot = {
	pipelines: TrayPipeline[];
	runningId: string | null;
	isRunning: boolean;
	busyId: string | null;
	error: string | null;
	theme: ThemePref;
};

export type TrayIntent = { kind: 'sync' } | { kind: 'run'; pipelineId: string } | { kind: 'stop' };
