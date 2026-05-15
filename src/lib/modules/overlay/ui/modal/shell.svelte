<script lang="ts">
	import type { Snippet } from 'svelte';
	import { fly, blur } from 'svelte/transition';
	import { backOut } from 'svelte/easing';

	interface Props {
		title?: string;
		titleClass?: string;
		canClose?: boolean;
		onClose?: () => void;
		badge?: Snippet;
		footer?: Snippet;
		children?: Snippet;
		zIndex?: number;
	}

	let {
		title,
		titleClass = 'text-md font-semibold text-neutral-1100',
		canClose = true,
		onClose,
		badge,
		footer,
		children,
		zIndex
	}: Props = $props();

	function onBackdrop() {
		if (canClose) onClose?.();
	}
</script>

<!-- svelte-ignore a11y_interactive_supports_focus a11y_click_events_have_key_events -->
<div
	class="fixed inset-0 z-100 flex items-center justify-center bg-black/1 backdrop-blur-sm p-6"
	style:z-index={zIndex}
	role="dialog"
	aria-modal="true"
	transition:blur|global={{ duration: 300, amount: 10 }}
	onclick={onBackdrop}
>
	<div
		class="flex max-h-[85vh] w-full max-w-2xl flex-col overflow-hidden rounded-2xl border border-neutral-400 bg-neutral-100 shadow-xl"
		transition:fly|global={{ duration: 400, y: 50, easing: backOut }}
		onclick={(e) => e.stopPropagation()}
		role="presentation"
	>
		{#if title || badge || canClose}
			<header class="flex items-center justify-between gap-3 border-b border-neutral-300 px-5 py-3">
				<h2 class={titleClass}>{title}</h2>
				<div class="flex items-center gap-2">
					{#if badge}{@render badge()}{/if}
					{#if canClose}
						<button
							type="button"
							class="rounded-md p-1 text-neutral-900 hover:bg-neutral-200"
							aria-label="Close"
							onclick={() => onClose?.()}
						>
							<svg class="h-4 w-4" viewBox="0 0 16 16" fill="currentColor" aria-hidden="true">
								<path d="M4.28 3.22a.75.75 0 0 0-1.06 1.06L6.94 8l-3.72 3.72a.75.75 0 1 0 1.06 1.06L8 9.06l3.72 3.72a.75.75 0 1 0 1.06-1.06L9.06 8l3.72-3.72a.75.75 0 0 0-1.06-1.06L8 6.94 4.28 3.22Z" />
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
			<footer class="flex items-center justify-end gap-2 border-t border-neutral-300 bg-neutral-200 px-5 py-3">
				{@render footer()}
			</footer>
		{/if}
	</div>
</div>
