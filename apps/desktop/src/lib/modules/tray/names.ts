import type { PipelineNode } from '@splitwave/nodes';

/** Display names seen since launch. A missing device or app has left the OS
 * lists and pipelines store only its id, so the name has to be remembered. */
export class NameMemory {
	private names = new Map<string, string>();

	remember(entries: Iterable<[id: string, name: string]>): void {
		for (const [id, name] of entries) this.names.set(id, name);
	}

	/** The remembered name, or the id itself for something never seen. */
	of(id: string): string {
		return this.names.get(id) ?? id;
	}
}

/** The id of what a source or speaker node asks the OS for, or its file name. */
export function wantedId(node: PipelineNode): string | null {
	const data = node.data as { deviceId?: string | null; bundleId?: string | null; filePath?: string | null };
	if (node.kind === 'speaker' || node.kind === 'microphone') return data.deviceId ?? null;
	if (node.kind === 'appAudio') return data.bundleId ?? null;
	if (node.kind === 'audioFile') return data.filePath?.split(/[\\/]/).pop() ?? null;
	return null;
}
