<script lang="ts">
	import { page } from '$app/state';
</script>

<div class="flex items-center gap-2">
	<a class:active={page.route.id === '/'} href="/" class="button-header px-4 text-sm">Pipelines</a>
	<a class:active={page.route.id === '/virtual-devices'} href="/virtual-devices" class="button-header px-4 text-sm">Virtual devices</a>
	<a class:active={page.route.id === '/wiki'} href="/wiki" class="button-header px-4 text-sm">Wiki</a>
	<a class:active={page.route.id === '/settings'} href="/settings" class="button-header px-4 text-sm">Settings</a>
</div>
