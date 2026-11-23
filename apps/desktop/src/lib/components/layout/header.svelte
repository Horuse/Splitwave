<script lang="ts">
	import { Button, Menu, MenuItem, MenuSeparator, Popover } from '@splitwave/ui';
	import type { Snippet } from 'svelte';
	import { slide } from 'svelte/transition';
	import { page } from '$app/state';
	import { platform } from '@tauri-apps/plugin-os';
	import { getCurrentWindow } from '@tauri-apps/api/window';
	import { modalManager } from '$lib/modules/overlay/modal';
	import { AboutModal } from '$lib/modules/about/ui';
	import { checkForUpdates } from '$lib/modules/updater';
	import { pipelineStore } from '$lib/modules/pipeline/stores.svelte';
	import { audioStore } from '$lib/modules/audio/stores.svelte';
	import { AnnouncementBanner } from '$lib/modules/announcements';

	interface Props {
		left?: Snippet;
		right?: Snippet;
	}

	let { left, right }: Props = $props();

	// macOS keeps its native menu + overlay traffic lights. Linux and Windows run
	// decoration-less, so the header carries the window controls and the menu
	// actions that would otherwise live in the native menu bar.
	const customTitlebar = platform() === 'linux' || platform() === 'windows';
	const win = customTitlebar ? getCurrentWindow() : null;

	// On macOS a page with nothing in the bar needs only a window drag area, not a visible header.
	const bare = $derived(!customTitlebar && !left && !right);

	let menuOpen = $state(false);

	function about() {
		menuOpen = false;
		modalManager.open('', AboutModal, { canClose: true });
	}
	function checkUpdates() {
		menuOpen = false;
		checkForUpdates().catch(() => {});
	}
	function undo() {
		menuOpen = false;
		pipelineStore.editorActions?.undo();
	}
	function redo() {
		menuOpen = false;
		pipelineStore.editorActions?.redo();
	}
</script>

{#if bare}
	<div data-tauri-drag-region class="absolute inset-x-0 top-0 z-10 h-8"></div>
{:else}
	<header
		data-tauri-drag-region
		class={[
			'z-500 flex w-full flex-row items-center gap-8 border-b border-theme/5 bg-background px-1.5',
			customTitlebar || page.route.id !== '/pipelines/[id]' ? 'h-10 pl-3' : 'h-[max(2.5rem,calc(40px/var(--ui-zoom,1)))] pl-[calc(80px/var(--ui-zoom,1))]'
		]}>
		{@render left?.()}

		<div class="ml-auto flex flex-row items-center gap-4">
			{@render right?.()}

			{#if customTitlebar}
				<Popover bind:open={menuOpen} placement="bottom-end" offsetPx={6}>
					{#snippet trigger()}
						<Button variant="ghost" icon type="button" aria-label="Menu">
							<svg class="size-4" fill="none" stroke="currentColor" stroke-width="1.6" viewBox="0 0 24 24">
								<path stroke-linecap="round" d="M4 7h16M4 12h16M4 17h16" />
							</svg>
						</Button>
					{/snippet}

					<Menu class="min-w-48">
						<MenuItem label="About Splitwave" onclick={about} />
						<MenuItem label="Check for Updates…" onclick={checkUpdates} />
						<MenuSeparator />
						<MenuItem label="Undo" onclick={undo} />
						<MenuItem label="Redo" onclick={redo} />
					</Menu>
				</Popover>

				<div class="flex items-center gap-0.5">
					<Button variant="ghost" icon aria-label="Minimize" onclick={() => win?.minimize()}>
						<svg class="size-4" fill="none" stroke="currentColor" stroke-width="1.6" viewBox="0 0 24 24">
							<path stroke-linecap="round" d="M5 12h14" />
						</svg>
					</Button>
					<Button variant="ghost" icon aria-label="Maximize" onclick={() => win?.toggleMaximize()}>
						<svg class="size-3.5" fill="none" stroke="currentColor" stroke-width="1.6" viewBox="0 0 24 24">
							<rect x="5" y="5" width="14" height="14" rx="2" />
						</svg>
					</Button>
					<Button variant="ghost" icon class="hover:!bg-red-500 hover:!text-white" aria-label="Close" onclick={() => win?.close()}>
						<svg class="size-4" fill="none" stroke="currentColor" stroke-width="1.6" viewBox="0 0 24 24">
							<path stroke-linecap="round" d="M6 6l12 12M18 6 6 18" />
						</svg>
					</Button>
				</div>
			{/if}
		</div>
	</header>
{/if}

{#if audioStore.safeMode}
	<div
		transition:slide={{ duration: 150 }}
		class="relative z-40 flex w-full items-center justify-between border-b border-amber-500/20 bg-amber-500/10 px-4 py-1.5 text-xs text-amber-600 dark:text-amber-400">
		<div class="flex items-center gap-2">
			<span class="rounded bg-amber-500/20 px-1.5 py-0.5 text-[10px] font-bold tracking-wider uppercase">Safe Mode</span>
			<span>Auto-starting audio pipeline was skipped because Splitwave crashed last time.</span>
		</div>
		<Button tone="warning" type="button" onclick={() => (audioStore.safeMode = false)}>Dismiss</Button>
	</div>
{/if}

<AnnouncementBanner />
