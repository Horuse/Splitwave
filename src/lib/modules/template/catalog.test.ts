import { describe, expect, test } from 'bun:test';
import { DEFAULT_NODE_DATA } from '$lib/modules/pipeline/defaults';
import { createMicrophoneArrayMember, microphoneArrayTopology } from '$lib/modules/pipeline/microphone_array';
import type { MicrophoneArrayNodeData, MicrophoneArraySource, SpeakerNodeData } from '$lib/modules/pipeline/types';
import { TEMPLATES } from './catalog';
import { instantiate } from './methods';

const TEMPLATE_IDS = ['blank', 'push-to-talk', 'voice-ducking', 'safe-game-audio', 'clean-mic', 'spatial_voice_multimic', 'full-voice-and-ducking'];

function spatialTemplate() {
	const template = TEMPLATES.find((candidate) => candidate.id === 'spatial_voice_multimic');
	if (!template) throw new Error('Spatial Voice template is missing');
	return template;
}

function arrayData(): MicrophoneArrayNodeData {
	return structuredClone(DEFAULT_NODE_DATA.microphoneArray);
}

describe('factory template catalog', () => {
	test('keeps stable unique ids and valid versions', () => {
		expect(TEMPLATES.map((template) => template.id)).toEqual(TEMPLATE_IDS);
		expect(new Set(TEMPLATES.map((template) => template.id)).size).toBe(TEMPLATES.length);
		for (const template of TEMPLATES) {
			expect(template.version).toBeGreaterThan(0);
			expect(Number.isInteger(template.version)).toBe(true);
			expect(template.name.trim().length).toBeGreaterThan(0);
			expect(template.description.trim().length).toBeGreaterThan(0);
		}
	});

	test('instantiates one setup-required array and an unassigned output', () => {
		const template = spatialTemplate();
		const pipeline = instantiate(template, template.name);
		const arrays = pipeline.nodes.filter((node) => node.kind === 'microphoneArray');
		expect(arrays).toHaveLength(1);
		const array = arrays[0].data as MicrophoneArrayNodeData;
		expect(array.sources).toEqual([]);
		expect(array.members).toEqual([]);
		expect(array.calibration.state).toBe('missing');
		expect(array.algorithm).toBe('delayAndSum');
		expect(pipeline.nodes.some((node) => node.kind === 'noiseSuppressor')).toBe(true);
		expect(pipeline.nodes.some((node) => node.kind === 'fileRecording')).toBe(false);
		const speaker = pipeline.nodes.find((node) => node.kind === 'speaker');
		expect((speaker?.data as SpeakerNodeData).deviceId).toBeNull();
		expect(pipeline.sourceTemplateId).toBe(template.id);
		expect(pipeline.sourceTemplateVersion).toBe(template.version);

		const kindById = new Map(pipeline.nodes.map((node) => [node.id, node.kind]));
		expect(pipeline.edges.map((edge) => [kindById.get(edge.source), kindById.get(edge.target)])).toEqual([
			['microphoneArray', 'noiseSuppressor'],
			['noiseSuppressor', 'compressor'],
			['compressor', 'eq'],
			['eq', 'speaker']
		]);
	});
});

describe('microphone array factory scenarios', () => {
	test('models a four-channel shared-clock source with one stream', () => {
		const source: MicrophoneArraySource = { id: 'device-a', deviceId: 'native-a', label: 'Device A' };
		const data = arrayData();
		data.sources = [source];
		data.masterSourceId = source.id;
		data.members = Array.from({ length: 4 }, (_, channel) => createMicrophoneArrayMember(source, channel, channel));
		expect(data.sources).toHaveLength(1);
		expect(microphoneArrayTopology(data)).toEqual({ microphones: 4, clockDomains: 1, slaveAsrcDomains: 0 });
	});

	test('models one slave ASRC for a mixed 4+2 topology', () => {
		const first: MicrophoneArraySource = { id: 'device-a', deviceId: 'native-a', label: 'Device A' };
		const second: MicrophoneArraySource = { id: 'device-b', deviceId: 'native-b', label: 'Device B' };
		const data = arrayData();
		data.sources = [first, second];
		data.masterSourceId = first.id;
		data.members = [
			...Array.from({ length: 4 }, (_, channel) => createMicrophoneArrayMember(first, channel, channel)),
			...Array.from({ length: 2 }, (_, channel) => createMicrophoneArrayMember(second, channel, channel + 4))
		];
		expect(data.sources).toHaveLength(2);
		expect(microphoneArrayTopology(data)).toEqual({ microphones: 6, clockDomains: 2, slaveAsrcDomains: 1 });
	});

	test('round-trips configured and missing devices without transient audition state', () => {
		const pipeline = instantiate(spatialTemplate(), 'Configured array');
		const node = pipeline.nodes.find((candidate) => candidate.kind === 'microphoneArray');
		if (!node) throw new Error('Microphone Array node is missing');
		const data = node.data as MicrophoneArrayNodeData;
		const first: MicrophoneArraySource = { id: 'device-a', deviceId: 'missing-native-a', label: 'Device A' };
		const second: MicrophoneArraySource = { id: 'device-b', deviceId: 'native-b', label: 'Device B' };
		data.sources = [first, second];
		data.masterSourceId = first.id;
		data.members = [
			...Array.from({ length: 4 }, (_, channel) => createMicrophoneArrayMember(first, channel, channel)),
			...Array.from({ length: 2 }, (_, channel) => createMicrophoneArrayMember(second, channel, channel + 4))
		];
		data.geometry = { kind: 'circular', radius_m: 0.08, rotation_degrees: 15 };
		data.calibration = { state: 'ready', fingerprint: 'array-v1-test', residualDelaySamples: 0.2, qualityScore: 94 };

		const encoded = JSON.stringify(pipeline);
		const decoded = JSON.parse(encoded) as typeof pipeline;
		const restored = decoded.nodes.find((candidate) => candidate.kind === 'microphoneArray')?.data as MicrophoneArrayNodeData;
		expect(restored.sources[0].deviceId).toBe('missing-native-a');
		expect(restored.members).toHaveLength(6);
		expect(restored.geometry).toEqual(data.geometry);
		expect(restored.calibration).toEqual(data.calibration);
		expect(encoded).not.toContain('auditionMode');
	});
});
