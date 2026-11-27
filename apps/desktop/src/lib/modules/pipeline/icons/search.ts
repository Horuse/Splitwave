const SUFFIX = '-20-filled';

export function iconLabel(name: string): string {
	return name.slice(0, -SUFFIX.length).replaceAll('-', ' ');
}

/** Every query word must appear in the name; names starting with the query come first. */
export function searchIcons(names: string[], query: string): string[] {
	const words = query
		.toLowerCase()
		.split(/[\s-]+/)
		.filter(Boolean);
	if (!words.length) return names;
	const first = words.join(' ');
	const matches = names.filter((name) => {
		const label = iconLabel(name);
		return words.every((word) => label.includes(word));
	});
	return [...matches.filter((n) => iconLabel(n).startsWith(first)), ...matches.filter((n) => !iconLabel(n).startsWith(first))];
}
