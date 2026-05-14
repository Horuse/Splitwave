import { methods } from './methods';
import type { NodeKind, Pipeline } from './types';

export type ClipboardEntry = {
	kind: NodeKind;
	data: Record<string, unknown>;
};

export type EditorActions = {
	addNode: (kind: NodeKind, position?: { x: number; y: number }) => void;
	addNodeWithData: (
		kind: NodeKind,
		data: Record<string, unknown>,
		position?: { x: number; y: number }
	) => void;
	getSnapshot: () => Pipeline | null;
	revertToSnapshot: (p: Pipeline) => void;
	undo: () => void;
	redo: () => void;
	canUndo: () => boolean;
	canRedo: () => boolean;
};

class PipelineStore {
	pipelines = $state<Pipeline[]>([]);
	editorActions = $state<EditorActions | null>(null);
	clipboard = $state<ClipboardEntry | null>(null);
	/** Wall-clock millis of the last successful auto-save. `0` before any. */
	lastSavedAt = $state<number>(0);

	async refresh(): Promise<void> {
		this.pipelines = await methods.list();
	}

	async save(p: Pipeline): Promise<void> {
		await methods.save(p);
		this.lastSavedAt = Date.now();
		await this.refresh();
	}

	async remove(id: string): Promise<void> {
		await methods.remove(id);
		await this.refresh();
	}
}

export const pipelineStore = new PipelineStore();
