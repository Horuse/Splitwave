import adapter from '@sveltejs/adapter-cloudflare';
import tailwindcss from '@tailwindcss/vite';
import Icons from 'unplugin-icons/vite';
import { enhancedImages } from '@sveltejs/enhanced-img';
import { sveltekit } from '@sveltejs/kit/vite';
import { mdsvex } from 'mdsvex';
import { defineConfig } from 'vite';

export default defineConfig({
	plugins: [
		enhancedImages(),
		tailwindcss(),
		Icons({ compiler: 'svelte' }),
		// Wiki articles are .svx files in @splitwave/wiki; headings get ids at build time
		// so anchors and the table of contents work in the prerendered HTML.
		mdsvex({ extensions: ['.svx'], heading_ids: true }),
		sveltekit({
			extensions: ['.svelte', '.svx'],
			compilerOptions: {
				// Force runes mode for the project, except for libraries. Can be removed in svelte 6.
				// mdsvex output (.svx) is compiled in its own mode, like libraries.
				runes: ({ filename }) =>
					filename.split(/[/\\]/).includes('node_modules') || filename.endsWith('.svx')
						? undefined
						: true
			},

			// adapter-auto only supports some environments, see https://svelte.dev/docs/kit/adapter-auto for a list.
			// If your environment is not supported, or you settled on a specific environment, switch out the adapter.
			// See https://svelte.dev/docs/kit/adapters for more information about adapters.
			adapter: adapter()
		})
	],
	// Packages that ship .svelte source: the Svelte plugin compiles them, esbuild cannot.
	optimizeDeps: {
		exclude: ['@xyflow/svelte', 'svelte-portal', 'svelte-floating-ui', '@number-flow/svelte']
	},
	server: { allowedHosts: ['test.splitwave.app'] }
});
