import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';
import Icons from 'unplugin-icons/vite';

export default defineConfig({
	plugins: [tailwindcss(), svelte(), Icons({ compiler: 'svelte' })],
	optimizeDeps: { exclude: ['svelte-portal', 'svelte-floating-ui', '@number-flow/svelte'] }
});
