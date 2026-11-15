<script lang="ts">
	import { onDestroy, type Snippet } from 'svelte';
	import { slide } from 'svelte/transition';
	import { cubicOut } from 'svelte/easing';
	import type { ClassValue, HTMLAnchorAttributes, HTMLButtonAttributes } from 'svelte/elements';
	import { smoothRadius } from '../attachments';
	import ButtonLoader from './button-loader.svelte';
	import { buttonVariants, type ButtonVariants } from './variants';

	type ClickEvent = MouseEvent & { currentTarget: EventTarget & HTMLElement };

	type Props = ButtonVariants &
		Omit<HTMLButtonAttributes, 'class' | 'onclick'> &
		Pick<HTMLAnchorAttributes, 'href' | 'target' | 'rel'> & {
			active?: boolean;
			class?: ClassValue;
			ref?: HTMLElement | null;
			children?: Snippet;
			/** With `loader`, a promise returned here keeps the button busy until it settles; resolving to false skips the check mark. */
			onclick?: (event: ClickEvent) => unknown;
			/** Automatic loader for an async onclick: blocks, spins, then flashes a check mark. */
			loader?: boolean;
			/** Manual loading state. */
			loading?: boolean;
			/** center replaces the label (width stays), start/end grow the button beside it. */
			loadingPosition?: 'center' | 'start' | 'end';
			/** Flash a check mark after a successful async onclick. */
			feedback?: boolean;
		};

	let {
		tone,
		variant,
		size,
		icon,
		active = false,
		class: className,
		ref = $bindable(null),
		href,
		type = 'button',
		disabled,
		children,
		onclick,
		loader = false,
		loading = false,
		loadingPosition = 'center',
		feedback = true,
		...rest
	}: Props = $props();

	const MIN_BUSY_MS = 450;

	let pending = $state(false);
	let succeeded = $state(false);
	let timer: ReturnType<typeof setTimeout> | undefined;

	const busy = $derived(loading || pending);
	const showLoader = $derived(busy || succeeded);
	const loaderState = $derived(succeeded && !busy ? 'success' : 'loading');
	const classes = $derived([buttonVariants({ tone, variant, size, icon }), className]);
	const radius = smoothRadius({ variable: '--r', maxRatio: 0.35 });

	async function handleClick(event: ClickEvent) {
		if (busy) {
			event.preventDefault();
			return;
		}
		clearTimeout(timer);
		succeeded = false;

		const result = onclick?.(event);
		if (!loader || typeof (result as Promise<unknown> | undefined)?.then !== 'function') return;

		pending = true;
		try {
			// A short minimum keeps the spinner from flashing when the work is instant.
			const [value] = await Promise.all([result, new Promise((r) => setTimeout(r, MIN_BUSY_MS))]);
			if (feedback && value !== false) {
				succeeded = true;
				timer = setTimeout(() => (succeeded = false), 1200);
			}
		} finally {
			pending = false;
		}
	}

	onDestroy(() => clearTimeout(timer));
</script>

{#snippet side()}
	{#if showLoader}
		<span class="flex" transition:slide={{ axis: 'x', duration: 220, easing: cubicOut }}><ButtonLoader state={loaderState} /></span>
	{/if}
{/snippet}

{#snippet content()}
	{#if loadingPosition === 'start'}{@render side()}{/if}
	<span class="btn-content">{@render children?.()}</span>
	{#if loadingPosition === 'end'}{@render side()}{/if}
	{#if showLoader && loadingPosition === 'center'}
		<ButtonLoader class="btn-loader-center" state={loaderState} />
	{/if}
{/snippet}

{#if href}
	<a
		bind:this={ref}
		{href}
		class={classes}
		data-active={active}
		data-loader={showLoader && loadingPosition === 'center' ? 'center' : undefined}
		aria-busy={busy || undefined}
		aria-disabled={disabled || undefined}
		onclick={handleClick}
		{@attach radius}
		{...rest as HTMLAnchorAttributes}>
		{@render content()}
	</a>
{:else}
	<button
		bind:this={ref}
		{type}
		{disabled}
		class={classes}
		data-active={active}
		data-loader={showLoader && loadingPosition === 'center' ? 'center' : undefined}
		aria-busy={busy || undefined}
		onclick={handleClick}
		{@attach radius}
		{...rest}>
		{@render content()}
	</button>
{/if}
