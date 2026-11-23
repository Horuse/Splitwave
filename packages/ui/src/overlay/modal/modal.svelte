<script lang="ts">
	import type { Snippet } from 'svelte';
	import { fly, fade } from 'svelte/transition';
	import { cubicOut } from 'svelte/easing';
	import Dismiss from '~icons/fluent/dismiss-16-filled';
	import { Button } from '../../button';
	import { smoothRadius } from '../../attachments';

	export type ModalSize = 'sm' | 'md' | 'lg' | 'xl';

	interface Props {
		title?: string;
		titleClass?: string;
		description?: string;
		size?: ModalSize;
		canClose?: boolean;
		onClose?: () => void;
		/** Leading mark beside the title, such as a tinted icon tile. */
		icon?: Snippet;
		badge?: Snippet;
		footer?: Snippet;
		children?: Snippet;
		zIndex?: number;
	}

	let {
		title,
		titleClass = 'text-sm font-semibold text-theme',
		description,
		size = 'md',
		canClose = true,
		onClose,
		icon,
		badge,
		footer,
		children,
		zIndex
	}: Props = $props();

	const WIDTH = {
		sm: 'max-w-sm',
		md: 'max-w-lg',
		lg: 'max-w-2xl',
		xl: 'max-w-4xl'
	} as const;

	function onBackdrop() {
		if (canClose) onClose?.();
	}
</script>

<!-- svelte-ignore a11y_interactive_supports_focus a11y_click_events_have_key_events -->
<div
	class="fixed inset-0 z-100 flex items-center justify-center bg-black/50 p-6"
	style:z-index={zIndex}
	role="dialog"
	aria-modal="true"
	transition:fade|global={{ duration: 150 }}
	onclick={onBackdrop}>
	<div
		data-layer="fg"
		class={['flex max-h-[85vh] w-full flex-col overflow-hidden border border-neutral-300 bg-foreground shadow-2xl', WIDTH[size]]}
		style="border-radius: var(--smooth-r, 20px)"
		{@attach smoothRadius({ variable: '--smooth-r' })}
		transition:fly|global={{ duration: 200, y: 8, easing: cubicOut }}
		onclick={(e) => e.stopPropagation()}
		role="presentation">
		{#if title || badge || canClose}
			<header class="flex items-start justify-between gap-3 px-5 pt-4">
				<div class="flex min-w-0 items-center gap-3">
					{#if icon}{@render icon()}{/if}
					<div class="flex min-w-0 flex-col gap-0.5">
						<h2 class={titleClass}>{title}</h2>
						{#if description}
							<p class="text-xs text-neutral-900">{description}</p>
						{/if}
					</div>
				</div>
				<div class="flex shrink-0 items-center gap-2">
					{#if badge}{@render badge()}{/if}
					{#if canClose}
						<Button icon variant="ghost" aria-label="Close" title="Close" onclick={() => onClose?.()}>
							<Dismiss class="size-4" />
						</Button>
					{/if}
				</div>
			</header>
		{/if}

		<div class="flex-1 overflow-y-auto">
			{@render children?.()}
		</div>

		{#if footer}
			<footer class="flex items-center justify-end gap-2 px-5 pt-2 pb-4">
				{@render footer()}
			</footer>
		{/if}
	</div>
</div>
