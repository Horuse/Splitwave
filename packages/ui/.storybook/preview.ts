import type { Preview } from '@storybook/svelte-vite';
import '../src/styles/app.css';

const preview: Preview = {
	tags: ['autodocs'],
	parameters: { docs: { toc: true } },
	globalTypes: {
		theme: {
			description: 'Theme',
			toolbar: {
				icon: 'circlehollow',
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
			if (!document.getElementById('overlays')) {
				const host = document.createElement('div');
				host.id = 'overlays';
				document.body.append(host);
			}
			document.documentElement.classList.toggle('dark', context.globals.theme === 'dark');
			return story();
		}
	]
};

export default preview;
