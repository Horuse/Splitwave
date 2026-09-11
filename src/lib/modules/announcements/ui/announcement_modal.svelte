<script lang="ts">
	import { modalManager, type ModalBaseProps } from '$lib/modules/overlay/modal';
	import Markdown from '$lib/components/markdown.svelte';
	import { announcementStore } from '../stores.svelte';
	import { handleAnnouncementAction } from '../methods';
	import type { Announcement } from '../types';

	interface Props extends ModalBaseProps {
		announcement: Announcement;
		totalModals?: number;
	}

	let { announcement, totalModals = 1, modalId }: Props = $props();

	let badgeClass = $derived.by(() => {
		if (announcement.severity === 'critical') return 'bg-red-500/20 text-red-600 dark:text-red-400';
		if (announcement.severity === 'warning') return 'bg-amber-500/20 text-amber-600 dark:text-amber-400';
		if (announcement.severity === 'success') return 'bg-emerald-500/20 text-emerald-600 dark:text-emerald-400';
		return 'bg-blue-500/20 text-blue-600 dark:text-blue-400';
	});

	let messageClass = $derived(announcement.severity === 'critical' ? 'font-medium text-red-600 dark:text-red-400' : 'text-neutral-900');

	function dismiss(): void {
		if (announcement.dismissible !== false) {
			announcementStore.dismiss(announcement.id);
		}
		if (modalId) {
			modalManager.close(modalId);
		}
	}

	async function performAction(): Promise<void> {
		try {
			await handleAnnouncementAction(announcement);
			if (announcement.action?.dismissOnClick) dismiss();
		} catch {}
	}
</script>

<div class="flex min-h-44 flex-col">
	<div class="flex flex-1 flex-col px-5 py-4">
		{#if announcement.badge || announcement.severity !== 'info'}
			<span class="mb-3 w-fit rounded-md px-2 py-0.5 text-[10px] font-semibold tracking-wide uppercase {badgeClass}">
				{announcement.badge ?? announcement.severity}
			</span>
		{/if}
		<p class="mb-3 text-xs leading-relaxed {messageClass}">{announcement.message}</p>

		{#if announcement.markdown}
			<Markdown
				source={announcement.markdown}
				class="max-h-80 overflow-auto rounded-lg border border-neutral-300 bg-neutral-200 p-3.5 text-xs leading-relaxed text-neutral-1100" />
		{/if}
	</div>

	<div class="flex items-center justify-end gap-2 px-5 pt-2 pb-4">
		<button type="button" class="button-main primary rounded-lg" onclick={dismiss}>
			{totalModals > 1 ? 'Next' : 'Close'}
		</button>

		{#if announcement.action}
			<button type="button" class="button-main secondary rounded-lg" onclick={performAction}>
				{announcement.action.label}
			</button>
		{/if}
	</div>
</div>
