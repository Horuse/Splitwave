import type { MicrophoneArrayMember, MicrophoneArrayNodeData, MicrophoneArraySource } from './types';

export function createMicrophoneArrayMember(source: MicrophoneArraySource, channelIndex: number, labelIndex: number): MicrophoneArrayMember {
	return {
		sourceId: source.id,
		channelIndex,
		label: `Mic ${labelIndex + 1}`,
		position: { x: 0, y: 0, z: 0 },
		enabled: true,
		weight: 1,
		gainDb: 0,
		polarityInverted: false,
		fixedDelaySamples: 0,
		quality: 'good',
		exclusionReason: null
	};
}

export function microphoneArrayTopology(data: Pick<MicrophoneArrayNodeData, 'sources' | 'members' | 'masterSourceId'>) {
	const activeSourceIds = new Set(data.members.map((member) => member.sourceId));
	const clockDomains = data.sources.filter((source) => activeSourceIds.has(source.id)).length;
	return {
		microphones: data.members.length,
		clockDomains,
		slaveAsrcDomains: Math.max(0, clockDomains - (data.masterSourceId ? 1 : 0))
	};
}
