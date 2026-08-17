import { modalManager } from '$lib/modules/overlay/modal';
import { audioStore } from './stores.svelte';
import { methods } from './methods';
import type { WindowsVirtualCableStatus } from './types';
import WindowsVirtualMicrophoneModal from './ui/windows_virtual_microphone_modal.svelte';

export async function chooseWindowsVirtualMicrophoneOutput(): Promise<string | null> {
	let status: WindowsVirtualCableStatus | null = null;
	try {
		status = await methods.windowsVirtualCableStatus();
	} catch {
		// The setup dialog owns status failures and presents them without leaking backend details.
	}

	if (!status?.usable || !status.renderEndpointName) {
		const title = status && status.state !== 'notInstalled' ? 'Finish virtual microphone setup' : 'Add virtual microphone';
		status =
			(await modalManager.open<WindowsVirtualCableStatus, { initialStatus: WindowsVirtualCableStatus | null; canClose: boolean; size: 'md' }>(
				title,
				WindowsVirtualMicrophoneModal,
				{
					initialStatus: status,
					canClose: false,
					size: 'md'
				}
			)) ?? null;
	}

	if (!status?.usable || !status.renderEndpointName) return null;
	await audioStore.refreshOutputDevices().catch(() => {});
	return status.renderEndpointName;
}
