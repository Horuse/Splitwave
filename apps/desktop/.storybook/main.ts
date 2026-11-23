import type { StorybookConfig } from '@storybook/sveltekit';

const CSF_ADDON = '@storybook/addon-svelte-csf';
const KIT_MOCKS = '@storybook/sveltekit/internal/mocks';
// Deps that ship uncompiled Svelte (components, runes). The Svelte plugin on
// this Vite cannot hook esbuild pre-bundling, so they are served unbundled.
const RAW_SVELTE_DEPS = [CSF_ADDON, KIT_MOCKS];
const isRawSvelte = (d: string) => RAW_SVELTE_DEPS.some((p) => d.startsWith(p));

const config: StorybookConfig & { optimizeViteDeps?: (deps: string[]) => string[] } = {
	stories: ['../src/**/*.stories.svelte'],
	addons: [CSF_ADDON],
	framework: '@storybook/sveltekit',
	optimizeViteDeps: (deps) => deps.filter((d) => !isRawSvelte(d)),
	viteFinal(config) {
		config.plugins = [
			...(config.plugins ?? []),
			{
				name: 'splitwave:unbundle-svelte-csf',
				enforce: 'post',
				config(c) {
					const deps = c.optimizeDeps ?? {};
					deps.include = (deps.include ?? []).filter((d) => !isRawSvelte(d));
					deps.exclude = [
						...(deps.exclude ?? []),
						CSF_ADDON,
						`${CSF_ADDON}/internal/create-runtime-stories`,
						...['forms', 'navigation', 'stores', 'state.svelte.js'].map((m) => `${KIT_MOCKS}/app/${m}`)
					];
					c.optimizeDeps = deps;
				}
			}
		];
		return config;
	}
};

export default config;
