<script lang="ts">
	// loading: a spinning arc. success: the ring closes and a check mark is drawn.
	let { state = 'loading', class: className }: { state?: 'loading' | 'success'; class?: string } = $props();
</script>

<svg
	viewBox="0 0 24 24"
	fill="none"
	stroke="currentColor"
	stroke-width="2.5"
	stroke-linecap="round"
	stroke-linejoin="round"
	aria-hidden="true"
	data-state={state}
	class={['btn-loader', className]}>
	<circle class="btn-loader-ring" cx="12" cy="12" r="9" />
	<path class="btn-loader-check" d="M7.5 12.5l3 3L16.5 9" />
</svg>
