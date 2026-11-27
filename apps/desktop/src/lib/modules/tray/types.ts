import type { IconSvg } from '$lib/modules/pipeline/icons/store.svelte';
import type { ThemePref } from '$lib/modules/theme/stores';

/** Main window to panel. */
export const TRAY_STATE_EVENT = 'tray://state';
/** Panel to main window. */
export const TRAY_INTENT_EVENT = 'tray://intent';

export type TrayStatus = 'idle' | 'running' | 'warning';

export type TrayPipeline = {
	id: string;
	name: string;
	/** Resolved by the main window, so the panel never loads the icon set. */
	icon: IconSvg | null;
	outputs: string[];
	/** Saved by a newer version, cannot be run. */
	stale: boolean;
};

export type TraySnapshot = {
	pipelines: TrayPipeline[];
	runningId: string | null;
	isRunning: boolean;
	/** Wall-clock millis the run started at. */
	startedAt: number | null;
	/** Devices, apps and files the running pipeline is waiting for. */
	waitingFor: string[];
	animatedNumbers: boolean;
	busyId: string | null;
	error: string | null;
	theme: ThemePref;
};

export type TrayIntent = { kind: 'sync' } | { kind: 'run'; pipelineId: string } | { kind: 'stop' };
