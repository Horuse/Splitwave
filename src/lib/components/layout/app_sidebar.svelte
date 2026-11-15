<script lang="ts">
	import { slide } from 'svelte/transition';
	import { cubicOut } from 'svelte/easing';
	import { NavTree, Sidebar, SidebarItem, SidebarSection, type NavNode } from '@splitwave/ui';
	import IconFlow from '~icons/fluent/flow-20-filled';
	import IconSpeaker from '~icons/fluent/speaker-2-20-filled';
	import IconBook from '~icons/fluent/book-20-filled';
	import IconHistory from '~icons/fluent/history-20-filled';
	import IconSettings from '~icons/fluent/settings-20-filled';
	import IconTheme from '~icons/fluent/dark-theme-20-filled';
	import { page } from '$app/state';
	import { tree, type WikiNode } from '@splitwave/wiki';
	import IconChevron from '~icons/fluent/chevron-right-16-filled';
	import { themeStore } from '$lib/modules/theme/stores';

	const toNav = (n: WikiNode): NavNode => ({ id: n.slug, label: n.title, href: `/wiki/${n.slug}`, children: n.children.map(toNav) });
	const wikiNav = tree.map(toNav);

	let wikiOpen = $state(false);
	$effect(() => {
		if (page.route.id?.startsWith('/wiki')) wikiOpen = true;
	});

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
	<SidebarItem label="Wiki" icon={IconBook} href="/wiki" active={page.route.id === '/wiki'}>
		{#snippet trailing()}
			<button
				type="button"
				class="sidebar-toggle"
				aria-label={wikiOpen ? 'Collapse wiki' : 'Expand wiki'}
				aria-expanded={wikiOpen}
				onclick={(e) => {
					e.preventDefault();
					e.stopPropagation();
					wikiOpen = !wikiOpen;
				}}>
				<IconChevron class={['size-3.5', wikiOpen && 'rotate-90']} />
			</button>
		{/snippet}
	</SidebarItem>
	{#if wikiOpen}
		<div transition:slide={{ duration: 220, easing: cubicOut }}>
			<NavTree items={wikiNav} activeId={page.params.slug} />
		</div>
	{/if}
	<SidebarItem label="Changelog" icon={IconHistory} href="/changelog" active={page.route.id === '/changelog'} />

	{#snippet footer()}
		<SidebarItem label="Theme: {$themeStore}" icon={IconTheme} onclick={cycleTheme} />
		<SidebarItem label="Settings" icon={IconSettings} href="/settings" active={page.route.id === '/settings'} />
	{/snippet}
</Sidebar>
