import type { StorybookConfig } from '@storybook/svelte-vite';

const config: StorybookConfig = {
	stories: ['../src/**/*.stories.svelte'],
	addons: ['@storybook/addon-docs', '@storybook/addon-svelte-csf', '@storybook/addon-a11y'],
	framework: '@storybook/svelte-vite'
};

export default config;
