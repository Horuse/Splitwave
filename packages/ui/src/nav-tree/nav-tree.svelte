<script lang="ts">
	import { slide } from 'svelte/transition';
	import { cubicOut } from 'svelte/easing';
	import IconChevron from '~icons/fluent/chevron-right-16-filled';
	import { SidebarGroup, SidebarItem } from '../sidebar';
	import NavTree from './nav-tree.svelte';
	import type { NavNode } from './types';

	let { items, activeId }: { items: NavNode[]; activeId?: string | null } = $props();

	let open = $state<Record<string, boolean>>({});

	const contains = (node: NavNode, id: string | null | undefined): boolean => !!id && (node.id === id || (node.children ?? []).some((c) => contains(c, id)));

	$effect(() => {
		for (const node of items) if (node.children?.length && contains(node, activeId) && node.id !== activeId) open[node.id] = true;
	});
</script>

<SidebarGroup activeKey={activeId}>
	{#each items as node (node.id)}
		<SidebarItem nested label={node.label} href={node.href} active={node.id === activeId}>
			{#snippet trailing()}
				{#if node.children?.length}
					<button
						type="button"
						class="sidebar-toggle"
						aria-label={open[node.id] ? `Collapse ${node.label}` : `Expand ${node.label}`}
						aria-expanded={!!open[node.id]}
						onclick={(e) => {
							e.preventDefault();
							e.stopPropagation();
							open[node.id] = !open[node.id];
						}}>
						<IconChevron class={['size-3.5', open[node.id] && 'rotate-90']} />
					</button>
				{/if}
			{/snippet}
		</SidebarItem>
		{#if node.children?.length && open[node.id]}
			<div transition:slide={{ duration: 220, easing: cubicOut }}>
				<NavTree items={node.children} {activeId} />
			</div>
		{/if}
	{/each}
</SidebarGroup>
