import { browser } from '$app/environment';
import { appSettings } from '$lib/modules/settings/stores.svelte';
import en from './locales/en';
import { DEFAULT_LOCALE, LOCALES, localeInfo } from './locales';

export type TranslateParams = Record<string, string | number>;

// The English file is the reference list, so it is imported directly rather
// than looked up: a renamed `DEFAULT_LOCALE` can then never crash the module.
const enMessages = en;

/**
 * Translates `key` into the language currently selected in Settings.
 *
 * `t()` is a function, not a store, so call sites inside `$derived` and
 * markup re-run when `appSettings.language` changes. Missing keys fall back to
 * English, then to the key itself, so a partially translated locale still
 * renders the whole UI.
 */
export function t(key: string, params?: TranslateParams): string {
	const messages = localeInfo(appSettings.language)?.messages ?? enMessages;
	const template = messages[key] ?? enMessages[key] ?? key;
	return params ? interpolate(template, params) : template;
}

function interpolate(template: string, params: TranslateParams): string {
	return template.replace(/\{(\w+)\}/g, (whole, name: string) => (Object.prototype.hasOwnProperty.call(params, name) ? String(params[name]) : whole));
}

/** Current language id, used for `lang` attributes and Intl formatting. */
export function currentLocale(): string {
	return localeInfo(appSettings.language)?.id ?? DEFAULT_LOCALE;
}

/** Keeps the document language in sync with the setting, for hyphenation,
 * font fallback and screen readers. Called from the root layout. */
export function applyDocumentLocale(): void {
	if (!browser) return;
	document.documentElement.lang = currentLocale();
}

export { LOCALES, DEFAULT_LOCALE, localeInfo };
