import type { MidiMessage } from '$lib/modules/pipeline/generated/MidiMessage';
import type { MidiBinding } from '$lib/modules/pipeline/types';
import { tauriListen } from '$lib/utils/tauri_event';

const MIDI_EVENT = 'midi://message';
/** Pedals and buttons send 0 or 127; a knob counts as on past halfway. */
const CC_ON = 64;
const NOTE_NAMES = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B'];

function matches(b: MidiBinding, m: MidiMessage): boolean {
	return b.device === m.device && b.channel === m.channel && b.kind === m.kind && b.number === m.number;
}

function isOn(m: MidiMessage): boolean {
	return m.kind === 'note' ? m.value > 0 : m.value >= CC_ON;
}

/** Middle C (60) is C4. */
export function formatMidi(b: MidiBinding): string {
	const control = b.kind === 'note' ? `${NOTE_NAMES[b.number % 12]}${Math.floor(b.number / 12) - 1}` : `CC ${b.number}`;
	return `${control} ch ${b.channel}`;
}

/** Follows the bound control while the calling component lives. While
 * `learning` is set, the next pressed note or moved CC becomes the binding. */
export function useMidiControl(
	binding: () => MidiBinding | undefined,
	learning: () => boolean,
	onLearn: (b: MidiBinding) => void,
	onState: (on: boolean) => void
) {
	tauriListen<MidiMessage>(MIDI_EVENT, (m) => {
		if (learning()) {
			if (isOn(m)) onLearn({ device: m.device, channel: m.channel, kind: m.kind, number: m.number });
			return;
		}
		const b = binding();
		if (b && matches(b, m)) onState(isOn(m));
	});
}
