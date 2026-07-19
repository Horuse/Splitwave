<script lang="ts">
	import type { Snippet } from 'svelte';
	import { fly, fade } from 'svelte/transition';
	import { cubicOut } from 'svelte/easing';

	export type ModalSize = 'sm' | 'md' | 'lg';

	interface Props {
		title?: string;
		titleClass?: string;
		description?: string;
		size?: ModalSize;
		canClose?: boolean;
		onClose?: () => void;
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
		badge,
		footer,
		children,
		zIndex
	}: Props = $props();

	const WIDTH = {
		sm: 'max-w-sm',
		md: 'max-w-lg',
		lg: 'max-w-2xl'
	} as const;

	function onBackdrop() {
		if (canClose) onClose?.();
	}
</script>

<!-- svelte-ignore a11y_interactive_supports_focus a11y_click_events_have_key_events -->
<div
	class="fixed inset-0 z-100 flex items-center justify-center bg-black/40 p-6 backdrop-blur-sm"
	style:z-index={zIndex}
	role="dialog"
	aria-modal="true"
	transition:fade|global={{ duration: 150 }}
	onclick={onBackdrop}
>
	<div
		class={[
			'flex max-h-[85vh] w-full flex-col overflow-hidden rounded-2xl border border-neutral-300 bg-neutral-100 shadow-2xl',
			WIDTH[size]
		]}
		transition:fly|global={{ duration: 200, y: 8, easing: cubicOut }}
		onclick={(e) => e.stopPropagation()}
		role="presentation"
	>
		{#if title || badge || canClose}
			<header class="flex items-start justify-between gap-3 px-5 pt-4">
				<div class="flex min-w-0 flex-col gap-0.5">
					<h2 class={titleClass}>{title}</h2>
					{#if description}
						<p class="text-xs text-neutral-900">{description}</p>
					{/if}
				</div>
				<div class="flex shrink-0 items-center gap-2">
					{#if badge}{@render badge()}{/if}
					{#if canClose}
						<button
							type="button"
							class="-mr-1 rounded-lg p-1 text-neutral-1000 transition-colors hover:bg-neutral-300"
							aria-label="Close"
							onclick={() => onClose?.()}
						>
							<svg class="size-4" viewBox="0 0 16 16" fill="currentColor" aria-hidden="true">
								<path
									d="M4.28 3.22a.75.75 0 0 0-1.06 1.06L6.94 8l-3.72 3.72a.75.75 0 1 0 1.06 1.06L8 9.06l3.72 3.72a.75.75 0 1 0 1.06-1.06L9.06 8l3.72-3.72a.75.75 0 0 0-1.06-1.06L8 6.94 4.28 3.22Z"
								/>
							</svg>
						</button>
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
