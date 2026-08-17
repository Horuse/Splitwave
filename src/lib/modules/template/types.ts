import type { AnyNodeData, NodeKind } from '$lib/modules/pipeline/types';

export interface TemplateNode {
	/** Local to the template; swapped for a cuid when the pipeline is built. */
	key: string;
	kind: NodeKind;
	position: { x: number; y: number };
	data?: Partial<AnyNodeData>;
}

export interface TemplateEdge {
	from: string;
	to: string;
	/** 1-based channel, matching the `chN` handle ids. */
	fromCh?: number;
	toCh?: number;
	/** Named target (a compressor sidechain) instead of a channel. */
	toHandle?: string;
}

export type TemplateAccent = 'neutral' | 'emerald' | 'sky' | 'violet' | 'amber' | 'rose';

export interface Template {
	id: string;
	version: number;
	accent: TemplateAccent;
	name: string;
	description: string;
	nodes: TemplateNode[];
	edges: TemplateEdge[];
}
