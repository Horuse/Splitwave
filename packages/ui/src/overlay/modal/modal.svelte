<script lang="ts">
	import type { Snippet } from 'svelte';
	import IconDismiss from '~icons/fluent/dismiss-16-filled';

	export type ModalSize = 'sm' | 'md' | 'lg' | 'xl';

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
		titleClass = 'text-sm font-semibold text-content',
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
		lg: 'max-w-2xl',
		xl: 'max-w-4xl'
	} as const;

	function onBackdrop() {
		if (canClose) onClose?.();
	}
</script>

<!-- svelte-ignore a11y_interactive_supports_focus a11y_click_events_have_key_events -->
<div class="fixed inset-0 z-100 flex items-center justify-center bg-black/50 p-6" style:z-index={zIndex} role="dialog" aria-modal="true" onclick={onBackdrop}>
	<div
		class={['flex max-h-[85vh] w-full flex-col overflow-hidden rounded-xl border border-line-strong bg-surface-1', WIDTH[size]]}
		onclick={(e) => e.stopPropagation()}
		role="presentation">
		{#if title || badge || canClose}
			<header class="flex items-start justify-between gap-3 px-5 pt-4">
				<div class="flex min-w-0 flex-col gap-0.5">
					<h2 class={titleClass}>{title}</h2>
					{#if description}
						<p class="text-xs text-muted">{description}</p>
					{/if}
				</div>
				<div class="flex shrink-0 items-center gap-2">
					{#if badge}{@render badge()}{/if}
					{#if canClose}
						<button
							type="button"
							class="-mr-1 rounded-lg p-1 text-muted hover:bg-surface-3 hover:text-content"
							aria-label="Close"
							onclick={() => onClose?.()}>
							<IconDismiss class="size-4" />
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
