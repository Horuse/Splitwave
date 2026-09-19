import { describe, expect, it } from 'bun:test';
import { parseMarkdown, type Inline } from '../src/lib/utils/markdown';

const text = (s: string): Inline => ({ kind: 'text', text: s });

describe('parseInline', () => {
	it('plain text stays a single text run', () => {
		expect(parseMarkdown('just words').length).toBe(1);
	});

	it('code spans', () => {
		const blocks = parseMarkdown('use `bun test` now');
		const p = blocks[0];
		if (p.kind !== 'paragraph') throw new Error('expected paragraph');
		expect(p.content[0]).toEqual(text('use '));
		expect(p.content[1]).toEqual({ kind: 'code', text: 'bun test' });
		expect(p.content[2]).toEqual(text(' now'));
	});

	it('bold and italic', () => {
		const blocks = parseMarkdown('**hot** and *cold*');
		const p = blocks[0];
		if (p.kind !== 'paragraph') throw new Error('expected paragraph');
		expect(p.content[0]).toEqual({ kind: 'strong', text: 'hot' });
		expect(p.content[1]).toEqual(text(' and '));
		expect(p.content[2]).toEqual({ kind: 'em', text: 'cold' });
	});

	it('markdown links and bare urls', () => {
		const blocks = parseMarkdown('[site](https://example.com) and https://plain.io');
		const p = blocks[0];
		if (p.kind !== 'paragraph') throw new Error('expected paragraph');
		expect(p.content[0]).toEqual({ kind: 'link', text: 'site', href: 'https://example.com' });
		expect(p.content[p.content.length - 1]).toEqual({
			kind: 'link',
			text: 'https://plain.io',
			href: 'https://plain.io'
		});
	});

	it('a single unmatched * becomes emphasis; trailing ** stays literal', () => {
		const blocks = parseMarkdown('a * b ** c');
		const p = blocks[0];
		if (p.kind !== 'paragraph') throw new Error('expected paragraph');
		expect(p.content[1]).toEqual({ kind: 'em', text: ' b ' });
		expect(p.content[p.content.length - 1]).toEqual(text('* c'));
	});
});

describe('parseMarkdown blocks', () => {
	it('headings carry levels', () => {
		const blocks = parseMarkdown('# one\n## two\n### three');
		expect(blocks.map((b) => (b.kind === 'heading' ? b.level : 0))).toEqual([1, 2, 3]);
	});

	it('bullet and ordered lists group separately', () => {
		const blocks = parseMarkdown('- a\n- b\n1. one\n2. two\n- c');
		const lists = blocks.filter((b) => b.kind === 'list');
		expect(lists.length).toBe(3);
		if (lists[0].kind === 'list') expect(lists[0].items).toEqual([[text('b')]].flat().length ? [[{ kind: 'text', text: 'a' }], [{ kind: 'text', text: 'b' }]] : []);
		const ordered = lists.find((l) => l.ordered);
		if (ordered?.kind === 'list') expect(ordered.items.map((i) => i[0])).toEqual([
			{ kind: 'text', text: 'one' },
			{ kind: 'text', text: 'two' }
		]);
	});

	it('a blank line closes a running list', () => {
		const blocks = parseMarkdown('- a\n\n- b');
		const lists = blocks.filter((b) => b.kind === 'list');
		expect(lists.length).toBe(2, 'blank line splits lists');
	});

	it('fenced code keeps its lines verbatim and resumes paragraphs after', () => {
		const blocks = parseMarkdown('before\n```\nconst a = 1;\n  keep *stars*\n```\nafter');
		expect(blocks).toEqual([
			{ kind: 'paragraph', content: [text('before')] },
			{ kind: 'code', text: 'const a = 1;\n  keep *stars*' },
			{ kind: 'paragraph', content: [text('after')] }
		]);
	});

	it('unterminated fence still emits code', () => {
		const blocks = parseMarkdown('```\nnever closed');
		expect(blocks).toEqual([{ kind: 'code', text: 'never closed' }]);
	});

	it('crlf is normalized', () => {
		const blocks = parseMarkdown('# t\r\n- x\r\n');
		expect(blocks.length).toBe(2);
	});

	it('headings accept inline markup', () => {
		const blocks = parseMarkdown('## Release **notes**');
		const h = blocks[0];
		if (h.kind !== 'heading') throw new Error('expected heading');
		expect(h.level).toBe(2);
		expect(h.content).toContainEqual({ kind: 'strong', text: 'notes' });
	});

	it('empty input yields nothing', () => {
		expect(parseMarkdown('')).toEqual([]);
	});
});
