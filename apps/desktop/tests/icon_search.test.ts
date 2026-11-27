import { describe, expect, test } from 'bun:test';
import { iconLabel, searchIcons } from '../src/lib/modules/pipeline/icons/search';

const NAMES = ['mic-20-filled', 'mic-off-20-filled', 'speaker-2-20-filled', 'comic-sans-20-filled', 'headphones-sound-wave-20-filled'];

describe('icon search', () => {
	test('label drops the style suffix', () => {
		expect(iconLabel('headphones-sound-wave-20-filled')).toBe('headphones sound wave');
	});

	test('an empty query keeps every icon', () => {
		expect(searchIcons(NAMES, '  ')).toEqual(NAMES);
	});

	test('names starting with the query come first', () => {
		expect(searchIcons(NAMES, 'mic')).toEqual(['mic-20-filled', 'mic-off-20-filled', 'comic-sans-20-filled']);
	});

	test('every word must match, in any order', () => {
		expect(searchIcons(NAMES, 'Wave headphones')).toEqual(['headphones-sound-wave-20-filled']);
	});

	test('the style suffix is not searchable', () => {
		expect(searchIcons(NAMES, 'filled')).toEqual([]);
	});
});
