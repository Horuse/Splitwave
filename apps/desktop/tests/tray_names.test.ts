import { describe, expect, test } from 'bun:test';
import type { PipelineNode } from '@splitwave/nodes';
import { NameMemory, wantedId } from '../src/lib/modules/tray/names';

const node = (kind: string, data: Record<string, unknown>) => ({ id: kind, kind, data }) as unknown as PipelineNode;

describe('NameMemory', () => {
	test('keeps the name of something that has since disappeared', () => {
		const names = new NameMemory();
		names.remember([['com.spotify.client', 'Spotify']]);
		names.remember([]);
		expect(names.of(wantedId(node('appAudio', { bundleId: 'com.spotify.client' }))!)).toBe('Spotify');
	});

	test('shows the id of something never seen', () => {
		expect(new NameMemory().of('alsa_input.usb-mic')).toBe('alsa_input.usb-mic');
	});
});

describe('wantedId', () => {
	test('reads the device, app or file a node asks for', () => {
		expect(wantedId(node('microphone', { deviceId: 'Fifine Microphone' }))).toBe('Fifine Microphone');
		expect(wantedId(node('audioFile', { filePath: '/music/take.wav' }))).toBe('take.wav');
		expect(wantedId(node('audioFile', { filePath: 'C:\\music\\take.wav' }))).toBe('take.wav');
		expect(wantedId(node('speaker', { deviceId: null }))).toBeNull();
	});
});
