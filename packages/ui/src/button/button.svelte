<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { HTMLAnchorAttributes, HTMLButtonAttributes } from 'svelte/elements';
	import { buttonVariants, type ButtonVariants } from './variants';

	type Props = ButtonVariants &
		Omit<HTMLButtonAttributes, 'class'> &
		Pick<HTMLAnchorAttributes, 'href' | 'target' | 'rel'> & {
			active?: boolean;
			class?: string;
			ref?: HTMLElement | null;
			children?: Snippet;
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
		...rest
	}: Props = $props();

	const classes = $derived([buttonVariants({ tone, variant, size, icon }), className]);
</script>

{#if href}
	<a
		bind:this={ref}
		{href}
		class={classes}
		data-active={active}
		aria-disabled={disabled || undefined}
		{...rest as HTMLAnchorAttributes}
	>
		{@render children?.()}
	</a>
{:else}
	<button bind:this={ref} {type} {disabled} class={classes} data-active={active} {...rest}>
		{@render children?.()}
	</button>
{/if}
