/** Parses the markdown subset GitHub release notes use into plain data.
 * Rendering is left to ReleaseNotes.svelte, so no HTML string is ever built and
 * Svelte escapes every value. */

export type Inline =
	| { kind: 'text'; text: string }
	| { kind: 'code'; text: string }
	| { kind: 'strong'; text: string }
	| { kind: 'em'; text: string }
	| { kind: 'break' }
	| { kind: 'link'; text: string; href: string };

export type Block =
	| { kind: 'heading'; content: Inline[] }
	| { kind: 'list'; items: Inline[][] }
	| { kind: 'quote'; content: Inline[] }
	| { kind: 'alert'; type: AlertType; content: Inline[] }
	| { kind: 'paragraph'; content: Inline[] };

export type AlertType = 'note' | 'tip' | 'important' | 'warning' | 'caution';

const REPO = 'https://github.com/Horuse/Splitwave';

// Repo links read as their path ("compare/v1.4.0...v1.4.1"), others without the scheme.
function shortUrl(url: string): string {
	return url.startsWith(`${REPO}/`) ? url.slice(REPO.length + 1) : url.replace(/^https?:\/\//, '');
}

// `code`, **strong**, *em*, [label](url), bare url, (#12) pull refs, @user mentions.
const INLINE =
	/`([^`]+)`|\*\*([^*]+)\*\*|\*([^*\s][^*]*)\*|\[([^\]]+)\]\((https?:\/\/[^\s)]+)\)|(https?:\/\/[^\s<)]+?)(?=[.,;:!?)]*(?:\s|$))|#(\d+)\b|(?<![\w/])@([A-Za-z0-9-]+)/g;

// Only http(s) becomes a link, so javascript: and data: URLs cannot slip into href.
function parseInline(line: string): Inline[] {
	const out: Inline[] = [];
	let last = 0;

	for (const m of line.matchAll(INLINE)) {
		const at = m.index;
		if (at > last) out.push({ kind: 'text', text: line.slice(last, at) });

		const [, code, strong, em, label, href, bare, pull, user] = m;
		if (code !== undefined) out.push({ kind: 'code', text: code });
		else if (strong !== undefined) out.push({ kind: 'strong', text: strong });
		else if (em !== undefined) out.push({ kind: 'em', text: em });
		else if (pull !== undefined)
			out.push({ kind: 'link', text: `#${pull}`, href: `${REPO}/pull/${pull}` });
		else if (user !== undefined)
			out.push({ kind: 'link', text: `@${user}`, href: `https://github.com/${user}` });
		else if (label !== undefined && href !== undefined)
			out.push({ kind: 'link', text: label, href });
		else if (bare !== undefined) out.push({ kind: 'link', text: shortUrl(bare), href: bare });

		last = at + m[0].length;
	}

	if (last < line.length) out.push({ kind: 'text', text: line.slice(last) });
	return out;
}

export function parseNotes(md: string): Block[] {
	const blocks: Block[] = [];
	let list: Inline[][] | null = null;

	const closeList = () => {
		if (list) {
			blocks.push({ kind: 'list', items: list });
			list = null;
		}
	};

	for (const raw of md.split('\n')) {
		const line = raw.trim();

		if (!line) {
			closeList();
			continue;
		}

		const heading = /^#{1,6}\s+(.*)$/.exec(line);
		if (heading) {
			closeList();
			blocks.push({ kind: 'heading', content: parseInline(heading[1]) });
			continue;
		}

		const bullet = /^[-*]\s+(.*)$/.exec(line);
		if (bullet) {
			list ??= [];
			list.push(parseInline(bullet[1]));
			continue;
		}

		// Consecutive `>` lines are one quote; each source line keeps its own line.
		const quote = /^>\s*(.*)$/.exec(line);
		if (quote) {
			closeList();
			const prev = blocks.at(-1);
			// GitHub alert marker: `> [!WARNING]` on the quote's first line.
			const alert = /^\[!(note|tip|important|warning|caution)\]$/i.exec(quote[1]);
			if (alert && prev?.kind !== 'quote' && prev?.kind !== 'alert')
				blocks.push({ kind: 'alert', type: alert[1].toLowerCase() as AlertType, content: [] });
			else if ((prev?.kind === 'quote' || prev?.kind === 'alert') && prev.content.length)
				prev.content.push({ kind: 'break' }, ...parseInline(quote[1]));
			else if (prev?.kind === 'alert') prev.content.push(...parseInline(quote[1]));
			else blocks.push({ kind: 'quote', content: parseInline(quote[1]) });
			continue;
		}

		closeList();
		blocks.push({ kind: 'paragraph', content: parseInline(line) });
	}

	closeList();
	return blocks;
}
