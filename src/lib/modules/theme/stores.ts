import { browser } from '$app/environment';
import { writable } from 'svelte/store';
import { withoutTransition } from '$lib/utils/transition';

export type ThemePref = 'light' | 'dark' | 'system';

const KEY = 'theme';

function stored(): ThemePref {
	if (!browser) return 'dark';
	const v = window.localStorage.getItem(KEY);
	return v === 'light' || v === 'dark' || v === 'system' ? v : 'dark';
}

const media = browser ? window.matchMedia('(prefers-color-scheme: dark)') : null;

function resolve(pref: ThemePref): 'light' | 'dark' {
	if (pref !== 'system') return pref;
	return media?.matches ? 'dark' : 'light';
}

function paint(pref: ThemePref): void {
	if (!browser) return;
	withoutTransition(() => (resolve(pref) === 'dark' ? document.documentElement.classList.add('dark') : document.documentElement.classList.remove('dark')));
}

export const themeStore = writable<ThemePref>(stored());

themeStore.subscribe((value) => {
	if (!browser) return;
	window.localStorage.setItem(KEY, value);
	paint(value);
});

// Only meaningful under 'system': the OS flipping themes must repaint live.
media?.addEventListener('change', () => {
	if (stored() === 'system') paint('system');
});
