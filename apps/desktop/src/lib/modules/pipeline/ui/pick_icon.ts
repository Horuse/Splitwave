import { modalManager } from '$lib/modules/overlay/modal';
import IconPickerModal from './icon_picker_modal.svelte';

export function pickPipelineIcon(current: string | undefined): Promise<string | null | undefined> {
	return modalManager.open<string | null>('Pipeline icon', IconPickerModal, { size: 'lg', current });
}
