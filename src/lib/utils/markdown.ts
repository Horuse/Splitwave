/** Markdown for release notes and announcements: CommonMark blocks plus the
 * GitHub extras people write (tables, task lists, strikethrough, callouts,
 * mermaid). Produces plain data -- rendering happens with regular Svelte
 * markup, so no HTML string from the source ever reaches the DOM. Raw HTML in
 * the source stays literal text. */

export type Inline =
	| { kind: 'text'; text: string }
	| { kind: 'code'; text: string }
	| { kind: 'strong'; children: Inline[] }
	| { kind: 'em'; children: Inline[] }
	| { kind: 'del'; children: Inline[] }
	| { kind: 'link'; href: string; children: Inline[] }
	| { kind: 'image'; src: string; alt: string; title?: string }
	| { kind: 'br' };

export type Align = 'left' | 'center' | 'right' | null;

export type CalloutKind = 'note' | 'tip' | 'important' | 'warning' | 'caution';

export interface ListItem {
	/** null for a plain item, a boolean for a task. */
	checked: boolean | null;
	blocks: Block[];
}

export type Block =
	| { kind: 'heading'; level: 1 | 2 | 3 | 4 | 5 | 6; content: Inline[] }
	| { kind: 'paragraph'; content: Inline[] }
	| { kind: 'hr' }
	| { kind: 'quote'; blocks: Block[] }
	| { kind: 'callout'; callout: CalloutKind; blocks: Block[] }
	| { kind: 'list'; ordered: boolean; start: number; items: ListItem[] }
	| { kind: 'code'; lang: string; text: string }
	| { kind: 'mermaid'; text: string }
	| { kind: 'table'; align: Align[]; head: Inline[][]; rows: Inline[][][] };

// ---------------------------------------------------------------- URLs

/** Links may leave the app for the web or mail; anything else stays text. */
export function safeHref(url: string): string | null {
	const u = url.trim();
	return /^(https?:\/\/|mailto:)/i.test(u) ? u : null;
}

/** Images load from the web only. */
export function safeSrc(url: string): string | null {
	const u = url.trim();
	return /^https?:\/\//i.test(u) ? u : null;
}

// ---------------------------------------------------------------- inline

const ESCAPABLE = '\\`*_{}[]()#+-.!|~<>';

/** Index of the `]` closing the `[` at `open`, honouring nesting and escapes. */
function closingBracket(s: string, open: number): number {
	let depth = 0;
	for (let i = open; i < s.length; i++) {
		const c = s[i];
		if (c === '\\') {
			i++;
		} else if (c === '`') {
			const end = s.indexOf('`', i + 1);
			if (end === -1) return -1;
			i = end;
		} else if (c === '[') {
			depth++;
		} else if (c === ']' && --depth === 0) {
			return i;
		}
	}
	return -1;
}

/** `(url "title")` right after a `]`: the url, optional title and where it ends. */
function destination(s: string, at: number): { url: string; title?: string; end: number } | null {
	if (s[at] !== '(') return null;
	const m = /^\(\s*<?([^\s>)]*)>?(?:\s+"([^"]*)")?\s*\)/.exec(s.slice(at));
	return m ? { url: m[1], title: m[2], end: at + m[0].length } : null;
}

/** Closing run of `marker` for an emphasis opened at `from`, or -1. */
function closingDelimiter(s: string, from: number, marker: string): number {
	for (let i = from; i < s.length; i++) {
		if (s[i] === '\\') {
			i++;
			continue;
		}
		if (s[i] === '`') {
			const end = s.indexOf('`', i + 1);
			if (end === -1) return -1;
			i = end;
			continue;
		}
		if (s.startsWith(marker, i) && !/\s/.test(s[i - 1] ?? ' ')) {
			// `**` must not close a `*` run and vice versa: skip longer runs of the same char.
			const ch = marker[0];
			let run = 0;
			while (s[i + run] === ch) run++;
			if (run === marker.length || (run > marker.length && marker.length === 3)) return i;
			i += run - 1;
		}
	}
	return -1;
}

function pushText(out: Inline[], text: string) {
	if (!text) return;
	const last = out[out.length - 1];
	if (last?.kind === 'text') last.text += text;
	else out.push({ kind: 'text', text });
}

export function parseInline(source: string): Inline[] {
	const out: Inline[] = [];
	const s = source;
	let i = 0;

	while (i < s.length) {
		const c = s[i];

		// Hard break: backslash or two spaces before a newline.
		if (c === '\n') {
			const before = out[out.length - 1];
			if (before?.kind === 'text' && / {2,}$/.test(before.text)) {
				before.text = before.text.replace(/ +$/, '');
				out.push({ kind: 'br' });
			} else {
				pushText(out, ' ');
			}
			i++;
			continue;
		}
		if (c === '\\') {
			if (s[i + 1] === '\n') {
				out.push({ kind: 'br' });
				i += 2;
				continue;
			}
			if (s[i + 1] && ESCAPABLE.includes(s[i + 1])) {
				pushText(out, s[i + 1]);
				i += 2;
				continue;
			}
		}

		if (c === '`') {
			let ticks = 0;
			while (s[i + ticks] === '`') ticks++;
			const fence = '`'.repeat(ticks);
			const end = s.indexOf(fence, i + ticks);
			if (end !== -1) {
				out.push({ kind: 'code', text: s.slice(i + ticks, end).replace(/^ (.*) $/, '$1') });
				i = end + ticks;
				continue;
			}
		}

		if (c === '!' && s[i + 1] === '[') {
			const close = closingBracket(s, i + 1);
			const dest = close !== -1 ? destination(s, close + 1) : null;
			if (dest) {
				const src = safeSrc(dest.url);
				const alt = s.slice(i + 2, close);
				if (src) out.push({ kind: 'image', src, alt, title: dest.title });
				else pushText(out, alt);
				i = dest.end;
				continue;
			}
		}

		if (c === '[') {
			const close = closingBracket(s, i);
			const dest = close !== -1 ? destination(s, close + 1) : null;
			if (dest) {
				const children = parseInline(s.slice(i + 1, close));
				const href = safeHref(dest.url);
				if (href) out.push({ kind: 'link', href, children });
				else out.push(...children);
				i = dest.end;
				continue;
			}
		}

		if (c === '<') {
			const m = /^<((?:https?:\/\/|mailto:)[^\s>]+)>/i.exec(s.slice(i));
			if (m) {
				out.push({ kind: 'link', href: m[1], children: [{ kind: 'text', text: m[1].replace(/^mailto:/i, '') }] });
				i += m[0].length;
				continue;
			}
		}

		if ((c === 'h' || c === 'H') && /^https?:\/\//i.test(s.slice(i, i + 8)) && !/[\w/]/.test(s[i - 1] ?? ' ')) {
			const m = /^https?:\/\/[^\s<]+/i.exec(s.slice(i))!;
			// Trailing punctuation belongs to the sentence, not the URL.
			const url = m[0].replace(/[.,;:!?)\]'"]+$/, '');
			out.push({ kind: 'link', href: url, children: [{ kind: 'text', text: url }] });
			i += url.length;
			continue;
		}

		if (c === '~' && s[i + 1] === '~') {
			const end = closingDelimiter(s, i + 2, '~~');
			if (end > i + 2) {
				out.push({ kind: 'del', children: parseInline(s.slice(i + 2, end)) });
				i = end + 2;
				continue;
			}
		}

		if (c === '*' || c === '_') {
			let run = 0;
			while (s[i + run] === c) run++;
			// `_` inside a word (snake_case) is not emphasis.
			const intraword = c === '_' && /\w/.test(s[i - 1] ?? ' ');
			const opens = !intraword && s[i + run] !== undefined && !/\s/.test(s[i + run]);
			if (opens) {
				const n = Math.min(run, 3);
				const marker = c.repeat(n);
				const end = closingDelimiter(s, i + n, marker);
				if (end > i + n) {
					const children = parseInline(s.slice(i + n, end));
					if (n === 3) out.push({ kind: 'strong', children: [{ kind: 'em', children }] });
					else out.push({ kind: n === 2 ? 'strong' : 'em', children });
					i = end + n;
					continue;
				}
			}
			pushText(out, c.repeat(run));
			i += run;
			continue;
		}

		// Plain run up to the next character that could start something.
		const next = s.slice(i + 1).search(/[\\`!\[<hH~*_\n]/);
		const stop = next === -1 ? s.length : i + 1 + next;
		pushText(out, s.slice(i, stop));
		i = stop;
	}

	return out;
}

// ---------------------------------------------------------------- blocks

const FENCE = /^ {0,3}(`{3,}|~{3,})\s*([\w+#.-]*)/;
const HEADING = /^ {0,3}(#{1,6})(?:\s+(.*?))?(?:\s+#+)?\s*$/;
const HR = /^ {0,3}([-*_])(?:\s*\1){2,}\s*$/;
const QUOTE = /^ {0,3}> ?(.*)$/;
const ITEM = /^( *)([-*+]|\d{1,9}[.)])\s+(.*)$/;
const TABLE_SEP = /^\s*\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)*\|?\s*$/;
const CALLOUT = /^\[!(note|tip|important|warning|caution)\]\s*$/i;

function splitRow(line: string): string[] {
	let row = line.trim();
	if (row.startsWith('|')) row = row.slice(1);
	if (row.endsWith('|') && !row.endsWith('\\|')) row = row.slice(0, -1);
	const cells: string[] = [];
	let cell = '';
	for (let i = 0; i < row.length; i++) {
		if (row[i] === '\\' && row[i + 1] === '|') {
			cell += '|';
			i++;
		} else if (row[i] === '|') {
			cells.push(cell.trim());
			cell = '';
		} else {
			cell += row[i];
		}
	}
	cells.push(cell.trim());
	return cells;
}

function alignOf(cell: string): Align {
	const c = cell.trim();
	const left = c.startsWith(':');
	const right = c.endsWith(':');
	return left && right ? 'center' : right ? 'right' : left ? 'left' : null;
}

function isBlockStart(line: string): boolean {
	return FENCE.test(line) || HEADING.test(line) || HR.test(line) || QUOTE.test(line) || ITEM.test(line);
}

function parseBlocks(lines: string[]): Block[] {
	const blocks: Block[] = [];
	let i = 0;

	while (i < lines.length) {
		const line = lines[i];

		if (!line.trim()) {
			i++;
			continue;
		}

		const fence = FENCE.exec(line);
		if (fence) {
			const marker = fence[1];
			const lang = fence[2].toLowerCase();
			const body: string[] = [];
			i++;
			while (i < lines.length && !new RegExp(`^ {0,3}${marker[0]}{${marker.length},}\\s*$`).test(lines[i])) {
				body.push(lines[i]);
				i++;
			}
			i++;
			const text = body.join('\n');
			blocks.push(lang === 'mermaid' ? { kind: 'mermaid', text } : { kind: 'code', lang, text });
			continue;
		}

		const heading = HEADING.exec(line);
		if (heading) {
			blocks.push({ kind: 'heading', level: heading[1].length as 1, content: parseInline(heading[2] ?? '') });
			i++;
			continue;
		}

		if (HR.test(line)) {
			blocks.push({ kind: 'hr' });
			i++;
			continue;
		}

		if (QUOTE.test(line)) {
			const inner: string[] = [];
			while (i < lines.length && lines[i].trim() && QUOTE.test(lines[i])) {
				inner.push(QUOTE.exec(lines[i])![1]);
				i++;
			}
			const callout = CALLOUT.exec(inner[0] ?? '');
			if (callout) {
				blocks.push({ kind: 'callout', callout: callout[1].toLowerCase() as CalloutKind, blocks: parseBlocks(inner.slice(1)) });
			} else {
				blocks.push({ kind: 'quote', blocks: parseBlocks(inner) });
			}
			continue;
		}

		if (ITEM.test(line)) {
			const [list, next] = parseList(lines, i);
			blocks.push(list);
			i = next;
			continue;
		}

		if (line.includes('|') && i + 1 < lines.length && TABLE_SEP.test(lines[i + 1]) && lines[i + 1].includes('-')) {
			const head = splitRow(line);
			const align = splitRow(lines[i + 1]).map(alignOf);
			const rows: Inline[][][] = [];
			i += 2;
			while (i < lines.length && lines[i].trim() && lines[i].includes('|')) {
				const cells = splitRow(lines[i]);
				rows.push(head.map((_, c) => parseInline(cells[c] ?? '')));
				i++;
			}
			blocks.push({ kind: 'table', align: head.map((_, c) => align[c] ?? null), head: head.map(parseInline), rows });
			continue;
		}

		// Paragraph: consecutive lines until a blank line or another block.
		const para: string[] = [line.trim()];
		i++;
		while (i < lines.length && lines[i].trim() && !isBlockStart(lines[i])) {
			// Keep the trailing spaces: two of them mark a hard break.
			para.push(lines[i].replace(/^\s+/, ''));
			i++;
		}
		blocks.push({ kind: 'paragraph', content: parseInline(para.join('\n')) });
	}

	return blocks;
}

/** A list starting at `start`; nested lists are the lines indented past the marker. */
function parseList(lines: string[], start: number): [Block, number] {
	const first = ITEM.exec(lines[start])!;
	const indent = first[1].length;
	const ordered = /\d/.test(first[2]);
	const list: Extract<Block, { kind: 'list' }> = {
		kind: 'list',
		ordered,
		start: ordered ? parseInt(first[2], 10) : 1,
		items: []
	};

	let i = start;
	while (i < lines.length) {
		const m = ITEM.exec(lines[i]);
		if (!m || m[1].length !== indent || /\d/.test(m[2]) !== ordered) break;

		// Content column: where the item's text starts; continuation lines sit at or past it.
		const contentCol = indent + m[2].length + 1;
		let text = m[3];
		let checked: boolean | null = null;
		const task = /^\[([ xX])\]\s+(.*)$/.exec(text);
		if (task) {
			checked = task[1] !== ' ';
			text = task[2];
		}

		const body: string[] = [text];
		i++;
		let blank = false;
		while (i < lines.length) {
			const l = lines[i];
			if (!l.trim()) {
				blank = true;
				body.push('');
				i++;
				continue;
			}
			const lead = l.length - l.trimStart().length;
			const sibling = ITEM.exec(l);
			if (lead >= Math.min(contentCol, indent + 2)) {
				body.push(l.slice(Math.min(lead, contentCol)));
				blank = false;
				i++;
			} else if (!blank && !sibling && !isBlockStart(l)) {
				// Lazy continuation of the item's paragraph.
				body.push(l.trim());
				i++;
			} else {
				break;
			}
		}
		while (body.length && !body[body.length - 1].trim()) body.pop();
		list.items.push({ checked, blocks: parseBlocks(body) });
		// A blank line before the next marker starts a new list, as the first parser did.
		if (blank) break;
	}

	return [list, i];
}

export function parseMarkdown(source: string): Block[] {
	return parseBlocks(source.replace(/\r\n?/g, '\n').replace(/\t/g, '    ').split('\n'));
}

/** Plain text of inline content, for previews and alt text. */
export function inlineText(parts: Inline[]): string {
	return parts
		.map((p) => {
			switch (p.kind) {
				case 'text':
				case 'code':
					return p.text;
				case 'image':
					return p.alt;
				case 'br':
					return ' ';
				default:
					return inlineText(p.children);
			}
		})
		.join('');
}
