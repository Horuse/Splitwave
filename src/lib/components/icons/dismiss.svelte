<script lang="ts">
	import svg from '@fluentui/svg-icons/icons/dismiss_16_filled.svg?raw';
	import Icon from './_icon.svelte';
	import type { ClassValue } from 'svelte/elements';
	let { class: cls = 'size-4', title }: { class?: ClassValue; title?: string } = $props();
</script>

<Icon {svg} class={cls} {title} />
