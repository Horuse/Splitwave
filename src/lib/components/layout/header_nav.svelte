<script lang="ts">
	import { page } from '$app/state';
	import { t } from '$lib/modules/i18n';
</script>

<div class="flex items-center gap-2">
	<a class:active={page.route.id === '/'} href="/" class="button-header px-4 text-sm">{t('nav.pipelines')}</a>
	<a class:active={page.route.id === '/virtual-devices'} href="/virtual-devices" class="button-header px-4 text-sm">{t('nav.virtualDevices')}</a>
	<a class:active={page.route.id === '/wiki'} href="/wiki" class="button-header px-4 text-sm">{t('nav.wiki')}</a>
	<a class:active={page.route.id === '/settings'} href="/settings" class="button-header px-4 text-sm">{t('nav.settings')}</a>
</div>
