import { describe, expect, it } from 'bun:test';
import {
	canConnect,
	channelHandle,
	channelOf,
	controlHandle,
	MAIN_HANDLE,
	parsePort,
	peerHandle,
	SIDECHAIN_HANDLE,
	signalOf
} from '../src/lib/modules/flow/utils/handles';

describe('handle builders', () => {
	it('prefix every handle with its signal', () => {
		expect(MAIN_HANDLE).toBe('audio:main');
		expect(SIDECHAIN_HANDLE).toBe('audio:sidechain');
		expect(channelHandle(3)).toBe('audio:ch3');
		expect(peerHandle('p')).toBe('audio:peer:p');
		expect(peerHandle('p', 0)).toBe('audio:peer:p:0');
		expect(controlHandle('power')).toBe('control:power');
	});
});

describe('parsePort', () => {
	it('reads what the builders write', () => {
		expect(parsePort(MAIN_HANDLE)).toEqual({ signal: 'audio', port: { kind: 'main' } });
		expect(parsePort(SIDECHAIN_HANDLE)).toEqual({ signal: 'audio', port: { kind: 'sidechain' } });
		expect(parsePort(channelHandle(12))).toEqual({ signal: 'audio', port: { kind: 'channel', ch: 12 } });
		expect(parsePort('audio:st2')).toEqual({ signal: 'audio', port: { kind: 'stereo', ch: 2 } });
		expect(parsePort(peerHandle('p'))).toEqual({ signal: 'audio', port: { kind: 'peer', peer: 'p', ch: null } });
		expect(parsePort(peerHandle('p', 0))).toEqual({ signal: 'audio', port: { kind: 'peer', peer: 'p', ch: 0 } });
		expect(parsePort(controlHandle('power'))).toEqual({ signal: 'control', name: 'power' });
	});

	it('rejects malformed handles', () => {
		for (const h of [
			'ch1',
			'main',
			'audio:',
			'audio:ch',
			'audio:ch0',
			'audio:mix',
			'audio:peer:',
			'audio:peer:p:x',
			'control:',
			'video:main',
			'',
			null,
			undefined
		]) {
			expect(parsePort(h)).toBeNull();
		}
	});
});

describe('channelOf', () => {
	it('reads channel handles and nothing else', () => {
		expect(channelOf(channelHandle(1))).toBe(1);
		expect(channelOf(SIDECHAIN_HANDLE)).toBeNull();
		expect(channelOf(peerHandle('p', 1))).toBeNull();
		expect(channelOf(controlHandle('ch1'))).toBeNull();
		expect(channelOf('ch1')).toBeNull();
	});
});

describe('canConnect', () => {
	it('joins sockets of one signal only', () => {
		expect(signalOf(channelHandle(1))).toBe('audio');
		expect(signalOf(controlHandle('power'))).toBe('control');
		expect(canConnect(channelHandle(1), SIDECHAIN_HANDLE)).toBe(true);
		expect(canConnect(controlHandle('out'), controlHandle('power'))).toBe(true);
		expect(canConnect(channelHandle(1), controlHandle('power'))).toBe(false);
		expect(canConnect(controlHandle('out'), MAIN_HANDLE)).toBe(false);
		expect(canConnect(null, MAIN_HANDLE)).toBe(false);
	});
});
