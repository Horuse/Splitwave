<script lang="ts">
	import '../app.css';
	import { page } from '$app/state';
	import AppShell from '$lib/components/layout/app_shell.svelte';

	let { children } = $props();
</script>

<!-- The tray panel is a second webview: mounting the shell there would start the audio engine's controller twice. -->
{#if page.route.id === '/tray'}
	{@render children()}
{:else}
	<AppShell {children} />
{/if}
