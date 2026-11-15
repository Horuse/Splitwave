import type { StorybookConfig } from '@storybook/svelte-vite';

const CSF_ADDON = '@storybook/addon-svelte-csf';

// The CSF addon ships uncompiled Svelte, so it must not go through esbuild pre-bundling.
const config: StorybookConfig & { optimizeViteDeps?: (deps: string[]) => string[] } = {
	stories: ['../src/**/*.stories.svelte'],
	addons: ['@storybook/addon-docs', CSF_ADDON, '@storybook/addon-a11y'],
	framework: '@storybook/svelte-vite',
	optimizeViteDeps: (deps) => deps.filter((d) => !d.startsWith(CSF_ADDON)),
	viteFinal(config) {
		config.plugins = [
			...(config.plugins ?? []),
			{
				name: 'splitwave:unbundle-svelte-csf',
				enforce: 'post',
				config(c) {
					const deps = c.optimizeDeps ?? {};
					deps.include = (deps.include ?? []).filter((d) => !d.startsWith(CSF_ADDON));
					deps.exclude = [...(deps.exclude ?? []), CSF_ADDON, `${CSF_ADDON}/internal/create-runtime-stories`];
					c.optimizeDeps = deps;
				}
			}
		];
		return config;
	}
};

export default config;
