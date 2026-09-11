<script lang="ts">
	import { slide } from 'svelte/transition';
	import { announcementStore } from '../stores.svelte';
	import { handleAnnouncementAction, openAnnouncementDetails } from '../methods';

	let active = $derived(announcementStore.activeBanner);
	let count = $derived(announcementStore.bannerCount);
	let currentIndex = $derived(announcementStore.currentBannerIndex);

	let badgeText = $derived(active?.badge ?? active?.severity.toUpperCase() ?? 'NOTICE');

	let containerClass = $derived.by(() => {
		switch (active?.severity) {
			case 'critical':
				return 'border-b border-red-500/30 bg-red-500/10 text-red-600 dark:text-red-400';
			case 'warning':
				return 'border-b border-amber-500/30 bg-amber-500/10 text-amber-600 dark:text-amber-400';
			case 'success':
				return 'border-b border-emerald-500/30 bg-emerald-500/10 text-emerald-600 dark:text-emerald-400';
			default:
				return 'border-b border-blue-500/30 bg-blue-500/10 text-blue-600 dark:text-blue-400';
		}
	});

	let badgeClass = $derived.by(() => {
		switch (active?.severity) {
			case 'critical':
				return 'bg-red-500/20 text-red-700 dark:text-red-300';
			case 'warning':
				return 'bg-amber-500/20 text-amber-700 dark:text-amber-300';
			case 'success':
				return 'bg-emerald-500/20 text-emerald-700 dark:text-emerald-300';
			default:
				return 'bg-blue-500/20 text-blue-700 dark:text-blue-300';
		}
	});

	let buttonClass = $derived.by(() => {
		switch (active?.severity) {
			case 'critical':
				return 'bg-red-500/20 hover:bg-red-500/30 text-red-700 dark:text-red-200';
			case 'warning':
				return 'bg-amber-500/20 hover:bg-amber-500/30 text-amber-700 dark:text-amber-200';
			case 'success':
				return 'bg-emerald-500/20 hover:bg-emerald-500/30 text-emerald-700 dark:text-emerald-200';
			default:
				return 'bg-blue-500/20 hover:bg-blue-500/30 text-blue-700 dark:text-blue-200';
		}
	});

	function onDismiss(): void {
		if (active) {
			announcementStore.dismiss(active.id);
		}
	}
</script>

{#if active}
	<div transition:slide={{ duration: 150 }} class="relative z-40 flex w-full items-center justify-between px-4 py-1.5 text-xs {containerClass}">
		<div class="flex min-w-0 flex-1 items-center gap-2.5 overflow-hidden">
			<span class="shrink-0 rounded px-1.5 py-0.5 text-[10px] font-bold tracking-wider uppercase {badgeClass}">
				{badgeText}
			</span>

			{#if count > 1}
				<div class="flex shrink-0 items-center gap-1 rounded bg-black/10 px-1.5 py-0.5 font-mono text-[11px] font-medium text-theme dark:bg-white/10">
					<button
						type="button"
						class="rounded px-1 hover:bg-black/10 dark:hover:bg-white/15"
						title="Previous notice"
						onclick={() => announcementStore.prevBanner()}>&lt;</button>
					<span>{currentIndex + 1}/{count}</span>
					<button
						type="button"
						class="rounded px-1 hover:bg-black/10 dark:hover:bg-white/15"
						title="Next notice"
						onclick={() => announcementStore.nextBanner()}>&gt;</button>
				</div>
			{/if}

			<div class="flex min-w-0 items-center gap-1.5 truncate">
				<strong class="shrink-0 font-medium">{active.title}:</strong>
				<span class="truncate opacity-90">{active.message}</span>
			</div>
		</div>

		<div class="flex shrink-0 items-center gap-2 pl-3">
			{#if active.type === 'both' || active.markdown}
				<button
					type="button"
					class="rounded px-2.5 py-0.5 text-xs font-medium transition-colors {buttonClass}"
					onclick={() => active && openAnnouncementDetails(active)}>
					Details
				</button>
			{/if}

			{#if active.action}
				<button
					type="button"
					class="rounded px-2.5 py-0.5 text-xs font-medium transition-colors {buttonClass}"
					onclick={() => active && handleAnnouncementAction(active)}>
					{active.action.label}
				</button>
			{/if}

			{#if active.dismissible !== false}
				<button type="button" class="rounded px-2 py-0.5 text-xs opacity-75 transition-opacity hover:opacity-100" onclick={onDismiss}> Dismiss </button>
			{/if}
		</div>
	</div>
{/if}
