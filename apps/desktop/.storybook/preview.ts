import type { Preview } from '@storybook/sveltekit';
import '../src/app.css';
import '@xyflow/svelte/dist/base.css';
import { DEFAULT_ENV, installMockBackend } from '../src/stories/harness/backend';

// Stories that never mount a node canvas still touch Tauri through shared stores.
installMockBackend(DEFAULT_ENV);
// The app shell never scrolls; story pages taller than the viewport must.
document.body.style.overflow = 'auto';

const preview: Preview = {
	globalTypes: {
		theme: {
			description: 'Colour theme',
			toolbar: {
				title: 'Theme',
				icon: 'mirror',
				items: [
					{ value: 'light', title: 'Light' },
					{ value: 'dark', title: 'Dark' }
				],
				dynamicTitle: true
			}
		}
	},
	initialGlobals: { theme: 'dark' },
	decorators: [
		(story, context) => {
			document.documentElement.classList.toggle('dark', context.globals.theme === 'dark');
			return story();
		}
	],
	parameters: {
		layout: 'fullscreen',
		controls: { expanded: true, sort: 'none' }
	}
};

export default preview;
