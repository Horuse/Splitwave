import { createId } from '@paralleldrive/cuid2';
import type { Pipeline, PipelineEdge, PipelineNode } from '../types';

/** A Mute carried its own global shortcut: `hotkey` toggled it, or with
 * `pushToTalk` unmuted it while held. Shortcuts are Key Input nodes now, wired
 * into the Mute's `open` input (high lets audio through), so each bound Mute
 * gets one: straight in for push-to-talk, through a Toggle for a toggle. */
const KEY_INPUT_OFFSET_X = 440;
const TOGGLE_OFFSET_X = 220;

interface LegacyMute {
	hotkey?: string;
	pushToTalk?: boolean;
	muted?: boolean;
}

function cable(source: string, sourceHandle: string, target: string, targetHandle: string): PipelineEdge {
	return { id: createId(), source, sourceHandle, target, targetHandle };
}

export function migrateMuteKeyInput(pipeline: Pipeline): Pipeline {
	const nodes: PipelineNode[] = [];
	const edges: PipelineEdge[] = [];
	for (const node of pipeline.nodes) {
		if (node.kind !== 'mute') {
			nodes.push(node);
			continue;
		}
		const { hotkey, pushToTalk, ...data } = node.data as typeof node.data & LegacyMute;
		nodes.push({ ...node, data } as PipelineNode);
		if (!hotkey) continue;

		const { x, y } = node.position;
		const key = createId();
		nodes.push({ id: key, kind: 'keyInput', data: { hotkey }, position: { x: x - KEY_INPUT_OFFSET_X, y } });
		if (pushToTalk) {
			edges.push(cable(key, 'control:out', node.id, 'control:open'));
			continue;
		}
		const toggle = createId();
		nodes.push({ id: toggle, kind: 'toggle', data: { on: !data.muted }, position: { x: x - TOGGLE_OFFSET_X, y } });
		edges.push(cable(key, 'control:out', toggle, 'control:in'), cable(toggle, 'control:out', node.id, 'control:open'));
	}
	return { ...pipeline, nodes, edges: [...pipeline.edges, ...edges] };
}
