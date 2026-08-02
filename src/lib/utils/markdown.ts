/** Minimal Markdown subset for release notes: headings, lists, bold, italic,
 * inline code, fenced code and links. Produces plain data -- rendering happens
 * with regular Svelte markup, so no HTML string ever reaches the DOM. */

export type Inline =
	| { kind: 'text'; text: string }
	| { kind: 'strong'; text: string }
	| { kind: 'em'; text: string }
	| { kind: 'code'; text: string }
	| { kind: 'link'; text: string; href: string };

export type Block =
	| { kind: 'heading'; level: number; content: Inline[] }
	| { kind: 'paragraph'; content: Inline[] }
	| { kind: 'list'; ordered: boolean; items: Inline[][] }
	| { kind: 'code'; text: string };

// One alternation per inline form; the surrounding text falls through as-is.
const INLINE =
	/(`[^`]+`)|(\*\*[^*]+\*\*)|(\*[^*\n]+\*)|(\[[^\]]+\]\(https?:\/\/[^\s)]+\))|(https?:\/\/[^\s<)]+)/g;

function parseInline(source: string): Inline[] {
	const out: Inline[] = [];
	let last = 0;

	for (const m of source.matchAll(INLINE)) {
		const start = m.index;
		if (start > last) out.push({ kind: 'text', text: source.slice(last, start) });
		const [token] = m;

		if (m[1]) {
			out.push({ kind: 'code', text: token.slice(1, -1) });
		} else if (m[2]) {
			out.push({ kind: 'strong', text: token.slice(2, -2) });
		} else if (m[3]) {
			out.push({ kind: 'em', text: token.slice(1, -1) });
		} else if (m[4]) {
			const split = token.indexOf('](');
			out.push({
				kind: 'link',
				text: token.slice(1, split),
				href: token.slice(split + 2, -1)
			});
		} else {
			out.push({ kind: 'link', text: token, href: token });
		}
		last = start + token.length;
	}

	if (last < source.length) out.push({ kind: 'text', text: source.slice(last) });
	return out;
}

export function parseMarkdown(source: string): Block[] {
	const blocks: Block[] = [];
	let list: { kind: 'list'; ordered: boolean; items: Inline[][] } | null = null;
	let fence: string[] | null = null;

	const closeList = () => {
		list = null;
	};
	const pushItem = (ordered: boolean, text: string) => {
		if (!list || list.ordered !== ordered) {
			list = { kind: 'list', ordered, items: [] };
			blocks.push(list);
		}
		list.items.push(parseInline(text));
	};

	for (const line of source.replace(/\r\n/g, '\n').split('\n')) {
		if (line.trimStart().startsWith('```')) {
			if (fence) {
				blocks.push({ kind: 'code', text: fence.join('\n') });
				fence = null;
			} else {
				closeList();
				fence = [];
			}
			continue;
		}
		if (fence) {
			fence.push(line);
			continue;
		}

		const heading = /^(#{1,6})\s+(.*)$/.exec(line);
		if (heading) {
			closeList();
			blocks.push({
				kind: 'heading',
				level: heading[1].length,
				content: parseInline(heading[2])
			});
			continue;
		}

		const bullet = /^\s*[-*+]\s+(.*)$/.exec(line);
		if (bullet) {
			pushItem(false, bullet[1]);
			continue;
		}

		const numbered = /^\s*\d+[.)]\s+(.*)$/.exec(line);
		if (numbered) {
			pushItem(true, numbered[1]);
			continue;
		}

		if (!line.trim()) {
			closeList();
			continue;
		}

		closeList();
		blocks.push({ kind: 'paragraph', content: parseInline(line) });
	}

	if (fence) blocks.push({ kind: 'code', text: fence.join('\n') });
	return blocks;
}
