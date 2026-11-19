import { describe, expect, test } from 'bun:test';
import { inlineText, parseInline, parseMarkdown, type Block } from '../src/lib/utils/markdown';

const para = (b: Block) => (b as Extract<Block, { kind: 'paragraph' }>).content;
const table = (b: Block) => b as Extract<Block, { kind: 'table' }>;

describe('inline', () => {
	test('emphasis forms', () => {
		expect(parseInline('**bold** *it* _it_ ~~gone~~ ***both***')).toEqual([
			{ kind: 'strong', children: [{ kind: 'text', text: 'bold' }] },
			{ kind: 'text', text: ' ' },
			{ kind: 'em', children: [{ kind: 'text', text: 'it' }] },
			{ kind: 'text', text: ' ' },
			{ kind: 'em', children: [{ kind: 'text', text: 'it' }] },
			{ kind: 'text', text: ' ' },
			{ kind: 'del', children: [{ kind: 'text', text: 'gone' }] },
			{ kind: 'text', text: ' ' },
			{ kind: 'strong', children: [{ kind: 'em', children: [{ kind: 'text', text: 'both' }] }] }
		]);
	});

	test('emphasis nests inside links and bold', () => {
		expect(parseInline('[see **docs**](https://a.dev)')).toEqual([
			{
				kind: 'link',
				href: 'https://a.dev',
				children: [{ kind: 'text', text: 'see ' }, { kind: 'strong', children: [{ kind: 'text', text: 'docs' }] }]
			}
		]);
		expect(parseInline('**a *b* c**')).toEqual([
			{ kind: 'strong', children: [{ kind: 'text', text: 'a ' }, { kind: 'em', children: [{ kind: 'text', text: 'b' }] }, { kind: 'text', text: ' c' }] }
		]);
	});

	test('snake_case and lone markers stay text', () => {
		expect(inlineText(parseInline('a snake_case_name and 2 * 3'))).toBe('a snake_case_name and 2 * 3');
		expect(parseInline('snake_case_name')).toEqual([{ kind: 'text', text: 'snake_case_name' }]);
	});

	test('code keeps markers literal', () => {
		expect(parseInline('`**x**` and ``a`b``')).toEqual([
			{ kind: 'code', text: '**x**' },
			{ kind: 'text', text: ' and ' },
			{ kind: 'code', text: 'a`b' }
		]);
	});

	test('escapes', () => {
		expect(parseInline('\\*not em\\*')).toEqual([{ kind: 'text', text: '*not em*' }]);
	});

	test('images and autolinks', () => {
		expect(parseInline('![Shot](https://x.dev/a.png "Title")')).toEqual([{ kind: 'image', src: 'https://x.dev/a.png', alt: 'Shot', title: 'Title' }]);
		expect(parseInline('go https://x.dev/page.')).toEqual([
			{ kind: 'text', text: 'go ' },
			{ kind: 'link', href: 'https://x.dev/page', children: [{ kind: 'text', text: 'https://x.dev/page' }] },
			{ kind: 'text', text: '.' }
		]);
		expect(parseInline('<mailto:a@b.dev>')).toEqual([{ kind: 'link', href: 'mailto:a@b.dev', children: [{ kind: 'text', text: 'a@b.dev' }] }]);
	});

	test('unsafe urls never become links or images', () => {
		expect(parseInline('[x](javascript:alert(1))')).not.toContainEqual(expect.objectContaining({ kind: 'link' }));
		expect(parseInline('![x](file:///etc/passwd)')).toEqual([{ kind: 'text', text: 'x' }]);
	});

	test('hard breaks', () => {
		expect(parseInline('a  \nb')).toEqual([{ kind: 'text', text: 'a' }, { kind: 'br' }, { kind: 'text', text: 'b' }]);
		expect(parseInline('a\nb')).toEqual([{ kind: 'text', text: 'a b' }]);
	});
});

describe('blocks', () => {
	test('headings, rules, multi-line paragraphs', () => {
		const blocks = parseMarkdown('# Title\n\nline one\nline two\n\n---\n### Sub ###');
		expect(blocks.map((b) => b.kind)).toEqual(['heading', 'paragraph', 'hr', 'heading']);
		expect(blocks[0]).toMatchObject({ level: 1 });
		expect(inlineText(para(blocks[1]))).toBe('line one line two');
		expect(blocks[3]).toMatchObject({ level: 3, content: [{ kind: 'text', text: 'Sub' }] });
	});

	test('fenced code keeps language; mermaid is its own block', () => {
		const blocks = parseMarkdown('```ts\nconst a = 1;\n```\n\n~~~mermaid\nflowchart LR\n  A --> B\n~~~');
		expect(blocks).toEqual([
			{ kind: 'code', lang: 'ts', text: 'const a = 1;' },
			{ kind: 'mermaid', text: 'flowchart LR\n  A --> B' }
		]);
	});

	test('nested lists and tasks', () => {
		const [list] = parseMarkdown('- [x] done\n- [ ] todo\n  - child\n  - child 2\n3. not this list');
		expect(list).toMatchObject({ kind: 'list', ordered: false });
		const items = (list as { items: { checked: boolean | null; blocks: { kind: string }[] }[] }).items;
		expect(items.map((it) => it.checked)).toEqual([true, false]);
		expect(items[1].blocks.map((b) => b.kind)).toEqual(['paragraph', 'list']);
	});

	test('ordered list start', () => {
		expect(parseMarkdown('3. a\n4. b')[0]).toMatchObject({ kind: 'list', ordered: true, start: 3 });
	});

	test('quotes nest blocks; github callouts', () => {
		const [quote, callout] = parseMarkdown('> a quote\n> - item\n\n> [!WARNING]\n> Careful');
		expect(quote).toMatchObject({ kind: 'quote' });
		expect((quote as { blocks: { kind: string }[] }).blocks.map((b) => b.kind)).toEqual(['paragraph', 'list']);
		expect(callout).toMatchObject({ kind: 'callout', callout: 'warning' });
	});

	test('tables with alignment and escaped pipes', () => {
		const [first] = parseMarkdown('| Name | Value |\n|:-----|------:|\n| a \\| b | **1** |');
		expect(first).toMatchObject({ kind: 'table', align: ['left', 'right'] });
		const t = table(first);
		expect(inlineText(t.head[0])).toBe('Name');
		expect(inlineText(t.rows[0][0])).toBe('a | b');
	});

	test('raw html stays text', () => {
		const [p] = parseMarkdown('<script>alert(1)</script>');
		expect(inlineText(para(p))).toBe('<script>alert(1)</script>');
	});
});

// Behaviour carried over from the first parser's tests, in the new data shape.
describe('kept from the first parser', () => {
	const t = (text: string) => ({ kind: 'text' as const, text });
	const p = (text: string) => ({ kind: 'paragraph', content: [t(text)] });
	const item = (text: string) => ({ checked: null, blocks: [p(text)] });

	test('plain text stays a single text run', () => {
		expect(parseMarkdown('just words')).toEqual([p('just words')] as Block[]);
	});

	test('bullet and ordered lists group separately', () => {
		const lists = parseMarkdown('- a\n- b\n1. one\n2. two\n- c').filter((b) => b.kind === 'list');
		expect(lists.length).toBe(3);
		expect(lists[0]).toMatchObject({ ordered: false, items: [item('a'), item('b')] });
		expect(lists[1]).toMatchObject({ ordered: true, items: [item('one'), item('two')] });
		expect(lists[2]).toMatchObject({ ordered: false, items: [item('c')] });
	});

	test('a blank line closes a running list', () => {
		expect(parseMarkdown('- a\n\n- b').filter((b) => b.kind === 'list').length).toBe(2);
	});

	test('fenced code keeps its lines verbatim and resumes paragraphs after', () => {
		expect(parseMarkdown('before\n```\nconst a = 1;\n  keep *stars*\n```\nafter')).toEqual([
			p('before'),
			{ kind: 'code', lang: '', text: 'const a = 1;\n  keep *stars*' },
			p('after')
		] as Block[]);
	});

	test('unterminated fence still emits code', () => {
		expect(parseMarkdown('```\nnever closed')).toEqual([{ kind: 'code', lang: '', text: 'never closed' }]);
	});

	test('crlf is normalized', () => {
		expect(parseMarkdown('# t\r\n- x\r\n')).toEqual([
			{ kind: 'heading', level: 1, content: [t('t')] },
			{ kind: 'list', ordered: false, start: 1, items: [item('x')] }
		] as Block[]);
	});

	test('headings accept inline markup', () => {
		const [h] = parseMarkdown('## Release **notes**');
		expect(h).toMatchObject({ kind: 'heading', level: 2 });
		expect((h as Extract<Block, { kind: 'heading' }>).content).toContainEqual({ kind: 'strong', children: [t('notes')] });
	});

	test('empty input yields nothing', () => {
		expect(parseMarkdown('')).toEqual([]);
	});
});
