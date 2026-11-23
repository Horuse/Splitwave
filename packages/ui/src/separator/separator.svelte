<script lang="ts">
	import type { ClassValue } from 'svelte/elements';

	let { vertical = false, class: className }: { vertical?: boolean; class?: ClassValue } = $props();
</script>

<hr class={['separator', vertical && 'separator-vertical', className]} aria-orientation={vertical ? 'vertical' : 'horizontal'} />
