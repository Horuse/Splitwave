/** Channels armed for one drag. Scoped to a single node, since a drag has one origin. */
class ChannelSelection {
	nodeId = $state<string | null>(null);
	channels = $state<number[]>([]);

	has(nodeId: string, ch: number): boolean {
		return this.nodeId === nodeId && this.channels.includes(ch);
	}

	toggle(nodeId: string, ch: number): void {
		if (this.nodeId !== nodeId) {
			this.nodeId = nodeId;
			this.channels = [ch];
			return;
		}
		this.channels = this.channels.includes(ch)
			? this.channels.filter((c) => c !== ch)
			: [...this.channels, ch].sort((a, b) => a - b);
		if (this.channels.length === 0) this.nodeId = null;
	}

	clear(): void {
		this.nodeId = null;
		this.channels = [];
	}
}

export const channelSelection = new ChannelSelection();

/** Target-side channel ceiling per node. Slots alone do not stop a multi-channel drag. */
export const channelCaps = new Map<string, number>();
