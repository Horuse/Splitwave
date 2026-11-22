<script lang="ts">
	import { page } from '$app/state';
	import { announcementStore } from '$lib/modules/announcements';
	import { changelogSeen } from '$lib/modules/changelog';

	let onAnnouncements = $derived(page.route.id?.startsWith('/announcements') ?? false);

	const TABS = [
		{ href: '/changelog', label: 'Changelog' },
		{ href: '/announcements', label: 'Announcements' }
	] as const;

	function unread(href: string): boolean {
		return href === '/changelog' ? changelogSeen.unread : announcementStore.unreadCount > 0;
	}
</script>

<nav class="flex gap-1 border-b border-theme/5">
	{#each TABS as tab (tab.href)}
		{@const active = tab.href === '/announcements' ? onAnnouncements : !onAnnouncements}
		<a
			href={tab.href}
			class={[
				'relative -mb-px flex items-center gap-2 border-b-2 px-3 py-2 text-sm',
				active ? 'border-neutral-1100 text-neutral-1200' : 'border-transparent text-neutral-800 hover:text-neutral-1100'
			]}>
			{tab.label}
			{#if unread(tab.href)}
				<span class="size-1.5 rounded-full bg-brand" aria-label="New"></span>
			{/if}
		</a>
	{/each}
</nav>
