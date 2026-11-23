<script lang="ts">
	export interface TocItem {
		id: string;
		label: string;
		level: number;
	}

	let { items, activeId, title = 'On this page' }: { items: TocItem[]; activeId?: string | null; title?: string } = $props();

	let list = $state<HTMLUListElement>();
	let top = $state(0);
	let height = $state(0);

	$effect(() => {
		void activeId;
		const el = list?.querySelector<HTMLElement>('[data-active="true"]');
		if (!el) return;
		top = el.offsetTop;
		height = el.offsetHeight;
	});
</script>

{#if items.length}
	<nav class="flex flex-col gap-2 text-xs">
		<span class="font-medium text-theme">{title}</span>
		<ul bind:this={list} class="relative flex flex-col border-l border-(--lv-3)">
			<span
				class="pointer-events-none absolute top-0 -left-px w-0.5 rounded-full bg-brand transition-[transform,height,opacity] duration-200 ease-out"
				class:opacity-0={!activeId}
				style:height="{height}px"
				style:transform="translateY({top}px)"></span>
			{#each items as item (item.id)}
				<li data-active={item.id === activeId}>
					<a
						href="#{item.id}"
						class={['block py-1 transition-colors duration-150 hover:text-theme', item.id === activeId ? 'text-theme' : 'text-neutral-900']}
						style:padding-left="{0.75 + (item.level - 2) * 0.75}rem">
						{item.label}
					</a>
				</li>
			{/each}
		</ul>
	</nav>
{/if}
