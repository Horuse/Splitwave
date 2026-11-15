<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import IconFlow from '~icons/fluent/flow-20-filled';
	import IconSpeaker from '~icons/fluent/speaker-2-20-filled';
	import IconBook from '~icons/fluent/book-20-filled';
	import IconHistory from '~icons/fluent/history-20-filled';
	import IconSettings from '~icons/fluent/settings-20-filled';
	import Sidebar from './sidebar.svelte';
	import SidebarItem from './sidebar-item.svelte';
	import SidebarSection from './sidebar-section.svelte';

	const { Story } = defineMeta({ title: 'Layout/Sidebar', component: Sidebar, tags: ['autodocs'], parameters: { layout: 'fullscreen' } });
</script>

<Story name="Default">
	{#snippet template()}
		<div class="flex h-[32rem] bg-background">
			<Sidebar>
				{#snippet header()}
					<div class="px-2.5 py-3 text-sm font-semibold">Splitwave</div>
				{/snippet}
				<SidebarItem label="Pipelines" icon={IconFlow} active />
				<SidebarItem label="Virtual devices" icon={IconSpeaker} />
				<SidebarSection label="Resources" />
				<SidebarItem label="Wiki" icon={IconBook} />
				<SidebarItem label="Changelog" icon={IconHistory} />
				{#snippet footer()}
					<SidebarItem label="Settings" icon={IconSettings} />
				{/snippet}
			</Sidebar>
		</div>
	{/snippet}
</Story>
