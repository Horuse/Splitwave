<script lang="ts">
	import { Button } from '@splitwave/ui';
	import { page } from '$app/state';
</script>

<div class="flex items-center gap-2">
	<Button variant="ghost" active={page.route.id === '/'} href="/">Pipelines</Button>
	<Button variant="ghost" active={page.route.id === '/virtual-devices'} href="/virtual-devices">Virtual devices</Button>
	<Button variant="ghost" active={page.route.id === '/wiki'} href="/wiki">Wiki</Button>
	<Button variant="ghost" active={page.route.id === '/settings'} href="/settings">Settings</Button>
</div>
