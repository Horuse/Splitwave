import { defineConfig } from 'vite';
import { sveltekit } from '@sveltejs/kit/vite';
import adapter from '@sveltejs/adapter-static';
import { mdsvex } from 'mdsvex';
import tailwindcss from '@tailwindcss/vite';
import Icons from 'unplugin-icons/vite';
import { pipelineIcons } from './vite_pipeline_icons.ts';

const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async () => ({
	plugins: [
		tailwindcss(),
		mdsvex({ extensions: ['.svx'], heading_ids: true }),
		sveltekit({
			extensions: ['.svelte', '.svx'],
			alias: { $lib: 'src/lib' },
			// Tauri has no Node.js server for SSR, so the app is an SPA with an index.html fallback.
			// See https://svelte.dev/docs/kit/single-page-apps and https://v2.tauri.app/start/frontend/sveltekit/
			adapter: adapter({ fallback: 'index.html' })
		}),
		Icons({ compiler: 'svelte' }),
		pipelineIcons()
	],

	// Packages that ship .svelte source -- let the Svelte plugin handle them
	// instead of esbuild pre-bundling (which has no .svelte loader).
	optimizeDeps: {
		exclude: ['@xyflow/svelte', 'svelte-portal', 'svelte-floating-ui', 'svelte-french-toast', '@number-flow/svelte']
	},

	// Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
	//
	// 1. prevent Vite from obscuring rust errors
	clearScreen: false,
	// 2. tauri expects a fixed port, fail if that port is not available
	server: {
		port: 1420,
		strictPort: true,
		host: host || false,
		hmr: host
			? {
					protocol: 'ws',
					host,
					port: 1421
				}
			: undefined,
		fs: {
			allow: ['../../packages']
		},
		watch: {
			// 3. tell Vite to ignore watching `src-tauri`
			ignored: ['**/src-tauri/**']
		}
	}
}));
