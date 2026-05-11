<script lang="ts">
	import { useAppStore } from '$lib/stores/app-store.svelte';

	const store = useAppStore();
	let busy = $state(false);

	async function toggle() {
		if (busy) return;
		busy = true;
		store.lastError = null;
		try {
			if (store.isRunning) {
				await store.audio.stopPipeline();
			} else {
				const snapshot = store.editorActions?.getSnapshot();
				if (!snapshot) {
					store.lastError = 'No pipeline loaded';
					return;
				}
				await store.audio.startPipeline({
					nodes: snapshot.nodes,
					edges: snapshot.edges
				});
			}
		} catch (e) {
			store.lastError = e instanceof Error ? e.message : String(e);
		} finally {
			busy = false;
		}
	}
</script>

<button
	class="rounded-md border px-3 py-1.5 text-sm font-medium disabled:opacity-50"
	class:bg-green-600={!store.isRunning}
	class:text-white={!store.isRunning}
	class:bg-red-600={store.isRunning}
	class:hover:bg-green-700={!store.isRunning}
	class:hover:bg-red-700={store.isRunning}
	disabled={busy}
	onclick={toggle}
>
	{#if busy}
		…
	{:else if store.isRunning}
		Stop
	{:else}
		Activate
	{/if}
</button>
