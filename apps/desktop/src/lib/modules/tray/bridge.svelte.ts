import { emitTo, listen, type UnlistenFn } from '@tauri-apps/api/event';
import { get } from 'svelte/store';
import { audioStore } from '$lib/modules/audio/stores.svelte';
import { pipelineStore } from '$lib/modules/pipeline/stores.svelte';
import { methods as pipelineMethods } from '$lib/modules/pipeline/methods';
import { isFromFuture } from '$lib/modules/pipeline/migrations';
import { appSettings } from '$lib/modules/settings/stores.svelte';
import { themeStore, type ThemePref } from '$lib/modules/theme/stores';
import { outputLabels, waitingLabels } from './outputs';
import { methods } from './methods';
import { NameMemory } from './names';
import { TRAY_INTENT_EVENT, TRAY_STATE_EVENT, type TrayIntent, type TraySnapshot } from './types';

/** Main-window side of the tray panel. All audio control stays in this window;
 * the panel only renders snapshots and sends intents. */
class TrayBridge {
	private busyId = $state<string | null>(null);
	private error = $state<string | null>(null);
	private theme = $state<ThemePref>(get(themeStore));

	private names = new NameMemory();

	snapshot(): TraySnapshot {
		this.names.remember([...audioStore.inputDevices, ...audioStore.outputDevices].map((d) => [d.id, d.name]));
		this.names.remember(audioStore.audioApplications.map((a) => [a.bundleId, a.name]));
		return {
			pipelines: pipelineStore.pipelines.map((p) => {
				const stale = isFromFuture(p);
				return { id: p.id, name: p.name, stale, outputs: stale ? [] : outputLabels(p.nodes, this.names) };
			}),
			runningId: audioStore.runningPipelineId,
			isRunning: audioStore.isRunning,
			startedAt: audioStore.startedAt,
			waitingFor: waitingLabels(audioStore.waitingNodes, this.names),
			animatedNumbers: appSettings.animatedNumbers,
			busyId: this.busyId,
			error: this.error,
			theme: this.theme
		};
	}

	publish(snapshot: TraySnapshot): void {
		emitTo('tray', TRAY_STATE_EVENT, snapshot).catch(() => {});
	}

	async start(): Promise<UnlistenFn> {
		const unsubscribeTheme = themeStore.subscribe((value) => (this.theme = value));
		const unlisten = await listen<TrayIntent>(TRAY_INTENT_EVENT, (event) => void this.handle(event.payload));
		return () => {
			unlisten();
			unsubscribeTheme();
		};
	}

	private async handle(intent: TrayIntent): Promise<void> {
		if (intent.kind === 'sync') {
			// Devices change while the app sits in the tray.
			await audioStore.refreshOutputDevices().catch(() => {});
			this.publish(this.snapshot());
			return;
		}
		if (this.busyId) return;
		this.busyId = intent.kind === 'run' ? intent.pipelineId : audioStore.runningPipelineId;
		this.error = null;
		try {
			if (intent.kind === 'stop') await audioStore.deactivatePipeline();
			else await this.run(intent.pipelineId);
		} catch (e) {
			this.error = e instanceof Error ? e.message : String(e);
			audioStore.reportError(e);
		} finally {
			this.busyId = null;
		}
	}

	private async run(pipelineId: string): Promise<void> {
		const p = await pipelineMethods.get(pipelineId);
		if (!p || isFromFuture(p)) throw new Error('This pipeline cannot be opened');
		await audioStore.activatePipeline(pipelineId, { nodes: p.nodes, edges: p.edges });
		// The open editor answers a choose-file request, so it has to be on screen.
		if (audioStore.chooseFileNodeId) await methods.showMainWindow();
	}
}

export const trayBridge = new TrayBridge();
