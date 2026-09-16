import { onDestroy, onMount } from 'svelte';
import { methods } from './methods';
import { audioStore } from './stores.svelte';
import { appSettings } from '$lib/modules/settings/stores.svelte';
import { formatHz } from '$lib/components/format';
import type { NativeDeviceInfo } from './types';

export type DeviceInfoTarget = { kind: 'input' | 'output'; deviceId: () => string | null } | { kind: 'system' | 'app'; pipelineRate?: () => number };

export interface DeviceInfoState {
	readonly info: NativeDeviceInfo | null;
	readonly isLoading: boolean;
	readonly sampleRate: number;
	readonly channels: number;
	readonly sampleFormat: string;
	readonly specText: string;
	readonly resamplingTooltip: string | undefined;
	refresh(): Promise<void>;
}

/**
 * Universally tracks the native sample rate, channels, and format of an audio
 * device or capture source, keeping it reactively up to date with hardware / OS
 * changes (e.g. sample rate changes in Audio MIDI Setup or Windows Sound Settings).
 */
export function useDeviceInfo(target: DeviceInfoTarget): DeviceInfoState {
	let info = $state<NativeDeviceInfo | null>(null);
	let isLoading = $state(true);
	let requestId = 0;
	let currentKey: string | null = null;

	async function query(): Promise<void> {
		const id = ++requestId;
		if ('deviceId' in target) {
			const deviceId = target.deviceId();
			if (!deviceId) {
				currentKey = null;
				info = null;
				isLoading = false;
				return;
			}
			const key = `${target.kind}:${deviceId}`;
			if (currentKey !== key) {
				currentKey = key;
				info = null;
			}
			isLoading = true;
			try {
				const r = await methods.deviceInfo(target.kind, deviceId);
				if (id === requestId && target.deviceId() === deviceId) {
					info = r;
				}
			} catch {
				if (id === requestId && target.deviceId() === deviceId) {
					info = null;
				}
			} finally {
				if (id === requestId) isLoading = false;
			}
		} else {
			const rate = target.pipelineRate ? target.pipelineRate() : appSettings.pipelineSampleRate;
			const key = `${target.kind}:${rate}`;
			if (currentKey !== key) {
				currentKey = key;
				info = null;
			}
			isLoading = true;
			try {
				const r = await methods.captureDeviceInfo(target.kind, rate);
				if (id === requestId) info = r;
			} catch {
				if (id === requestId) info = null;
			} finally {
				if (id === requestId) isLoading = false;
			}
		}
	}

	// Re-run whenever reactive dependencies change
	$effect(() => {
		if ('deviceId' in target) {
			const id = target.deviceId();
			const _devs = target.kind === 'input' ? audioStore.inputDevices : audioStore.outputDevices;
			const _running = audioStore.isRunning;
			if (!id) {
				info = null;
				isLoading = false;
				return;
			}
			void query();
		} else {
			const _devs = audioStore.outputDevices;
			const _running = audioStore.isRunning;
			const _rate = target.pipelineRate ? target.pipelineRate() : appSettings.pipelineSampleRate;
			void query();
		}
	});

	let timer: ReturnType<typeof setInterval> | undefined;

	onMount(() => {
		const onRefresh = () => {
			void query();
		};

		if (typeof window !== 'undefined') {
			window.addEventListener('focus', onRefresh);
		}
		if (typeof document !== 'undefined') {
			document.addEventListener('visibilitychange', onRefresh);
			timer = setInterval(() => {
				if (document.visibilityState === 'visible') {
					void query();
				}
			}, 2500);
		}

		return () => {
			if (typeof window !== 'undefined') {
				window.removeEventListener('focus', onRefresh);
			}
			if (typeof document !== 'undefined') {
				document.removeEventListener('visibilitychange', onRefresh);
			}
			if (timer) clearInterval(timer);
		};
	});

	onDestroy(() => {
		if (timer) clearInterval(timer);
	});

	const sampleRate = $derived(info?.sampleRate ?? 48_000);
	const channels = $derived(info?.channels ?? 2);
	const sampleFormat = $derived(info?.sampleFormat ?? 'f32');
	const specText = $derived(`${formatHz(sampleRate)} · ${channels} ch · ${sampleFormat}`);

	const resamplingTooltip = $derived.by(() => {
		if (!info) return undefined;
		if (target.kind === 'output') {
			if (info.sampleRate === appSettings.pipelineSampleRate) return undefined;
			return `Resampling: ${formatHz(appSettings.pipelineSampleRate)} → ${formatHz(info.sampleRate)}`;
		} else {
			const sr = info.sampleRate;
			if (sr === appSettings.pipelineSampleRate) return undefined;
			return `Resampling: ${formatHz(sr)} → ${formatHz(appSettings.pipelineSampleRate)}`;
		}
	});

	return {
		get info() {
			return info;
		},
		get isLoading() {
			return isLoading;
		},
		get sampleRate() {
			return sampleRate;
		},
		get channels() {
			return channels;
		},
		get sampleFormat() {
			return sampleFormat;
		},
		get specText() {
			return specText;
		},
		get resamplingTooltip() {
			return resamplingTooltip;
		},
		refresh: query
	};
}
