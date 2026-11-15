<script lang="ts">
	import type { ClassValue, HTMLInputAttributes } from 'svelte/elements';
	import { inputVariants, type InputVariants } from './variants';

	type Props = InputVariants &
		Omit<HTMLInputAttributes, 'class' | 'size'> & {
			invalid?: boolean;
			class?: ClassValue;
			ref?: HTMLInputElement | null;
		};

	let { size, mono, invalid = false, value = $bindable(), class: className, ref = $bindable(null), ...rest }: Props = $props();
</script>

<input bind:this={ref} bind:value aria-invalid={invalid || undefined} class={[inputVariants({ size, mono }), className]} {...rest} />
