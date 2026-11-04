import en from './locales/en';
import zhCN from './locales/zh-CN';

/** Every locale ships as a flat `key -> text` map. Keys are stable identifiers;
 * the English file is the reference list a new locale must cover. */
export type LocaleMessages = Record<string, string>;

export interface LocaleInfo {
	id: string;
	/** Endonym, shown in the picker in the language's own script. */
	label: string;
	messages: LocaleMessages;
}

export const LOCALES: LocaleInfo[] = [
	{ id: 'en', label: 'English', messages: en },
	{ id: 'zh-CN', label: '简体中文', messages: zhCN }
];

export const DEFAULT_LOCALE = 'en';

const byId = new Map(LOCALES.map((l) => [l.id, l]));

export function localeInfo(id: string): LocaleInfo | undefined {
	return byId.get(id);
}
