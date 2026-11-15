<script lang="ts">
	import { Sidebar, SidebarItem, SidebarSection } from '@splitwave/ui';
	import IconFlow from '~icons/fluent/flow-20-regular';
	import IconSpeaker from '~icons/fluent/speaker-2-20-regular';
	import IconBook from '~icons/fluent/book-20-regular';
	import IconHistory from '~icons/fluent/history-20-regular';
	import IconSettings from '~icons/fluent/settings-20-regular';
	import IconTheme from '~icons/fluent/dark-theme-20-regular';
	import { page } from '$app/state';
	import { themeStore } from '$lib/modules/theme/stores';

	function cycleTheme() {
		$themeStore = $themeStore === 'light' ? 'dark' : $themeStore === 'dark' ? 'system' : 'light';
	}
</script>

<Sidebar>
	{#snippet header()}
		<div data-tauri-drag-region class="h-8 shrink-0"></div>
		<div class="flex items-center gap-2 px-2.5 pb-2">
			<img src="/icon.svg" alt="" class="pointer-events-none size-5" />
			<span class="pointer-events-none text-sm font-semibold">Splitwave</span>
		</div>
	{/snippet}

	<SidebarSection label="Workspace" />
	<SidebarItem label="Pipelines" icon={IconFlow} href="/" active={page.route.id === '/'} />
	<SidebarItem label="Virtual devices" icon={IconSpeaker} href="/virtual-devices" active={page.route.id === '/virtual-devices'} />
	<SidebarSection label="Resources" />
	<SidebarItem label="Wiki" icon={IconBook} href="/wiki" active={page.route.id === '/wiki'} />
	<SidebarItem label="Changelog" icon={IconHistory} href="/changelog" active={page.route.id === '/changelog'} />

	{#snippet footer()}
		<SidebarItem label="Theme: {$themeStore}" icon={IconTheme} onclick={cycleTheme} />
		<SidebarItem label="Settings" icon={IconSettings} href="/settings" active={page.route.id === '/settings'} />
	{/snippet}
</Sidebar>
