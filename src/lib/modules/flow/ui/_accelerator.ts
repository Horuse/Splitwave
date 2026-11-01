const MODIFIER_CODES = new Set(['ControlLeft', 'ControlRight', 'ShiftLeft', 'ShiftRight', 'AltLeft', 'AltRight', 'MetaLeft', 'MetaRight', 'CapsLock']);

/**
 * Tauri parses accelerators as `keyboard_types::Code` names, which is exactly
 * what `KeyboardEvent.code` reports. Returns null while only modifiers are held.
 */
export function accelerator(e: KeyboardEvent): string | null {
	if (MODIFIER_CODES.has(e.code)) return null;

	const parts: string[] = [];
	if (e.metaKey) parts.push('Command');
	if (e.ctrlKey) parts.push('Control');
	if (e.altKey) parts.push('Alt');
	if (e.shiftKey) parts.push('Shift');
	parts.push(e.code);
	return parts.join('+');
}

export function formatAccelerator(combo: string): string {
	return combo
		.split('+')
		.map((part) => {
			if (part === 'Command') return '⌘';
			if (part === 'Control') return '⌃';
			if (part === 'Alt') return '⌥';
			if (part === 'Shift') return '⇧';
			if (part.startsWith('Key')) return part.slice(3);
			if (part.startsWith('Digit')) return part.slice(5);
			if (part.startsWith('Numpad')) return `Num ${part.slice(6)}`;
			if (part.startsWith('Arrow')) return part.slice(5);
			return part;
		})
		.join(' ');
}
