import { browser } from '$app/environment';

import { writable } from 'svelte/store';

// eslint-disable-next-line @typescript-eslint/no-explicit-any
import { withoutTransition } from '$lib/utils/transition';

const themeStorage = browser ? window.localStorage.getItem('theme') : 'dark';

// Writable store to manage the theme
export const themeStore = writable<string>(themeStorage || 'dark');

// Subscribe to theme changes and update localStorage and document class accordingly
themeStore.subscribe((value) => {
	if (browser) {
		window.localStorage.setItem('theme', value);

		withoutTransition(() =>
			value == 'dark'
				? document.documentElement.classList.add('dark')
				: document.documentElement.classList.remove('dark')
		);
	}
});
