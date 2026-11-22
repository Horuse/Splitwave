import { methods } from './methods';
import { audioStore } from './stores.svelte';
import * as ui from './ui';
import type * as types from './types';

const Audio = {
	methods,
	stores: { audio: audioStore },
	ui
};

export { useDeviceInfo, type DeviceInfoTarget, type DeviceInfoState } from './device_info.svelte';
export type { types };
export default Audio;
