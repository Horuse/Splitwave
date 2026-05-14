export type NodeActionName = 'refresh' | 'resetPeaks' | 'chooseFile';

interface NodeActionDetail {
	nodeId: string;
	action: NodeActionName;
}

const EVENT_NAME = 'node-action';

export function emitNodeAction(nodeId: string, action: NodeActionName): void {
	window.dispatchEvent(new CustomEvent<NodeActionDetail>(EVENT_NAME, { detail: { nodeId, action } }));
}

export function onNodeAction(nodeId: string, action: NodeActionName, fn: () => void): () => void {
	const handler = (e: Event) => {
		const ce = e as CustomEvent<NodeActionDetail>;
		if (ce.detail.nodeId === nodeId && ce.detail.action === action) fn();
	};
	window.addEventListener(EVENT_NAME, handler);
	return () => window.removeEventListener(EVENT_NAME, handler);
}
