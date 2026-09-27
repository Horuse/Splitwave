<script lang="ts">
	import { untrack, type Snippet } from 'svelte';
	import { installMockBackend, type MockEnv } from './backend';

	let { env, children }: { env: MockEnv; children: Snippet } = $props();

	// Installed during init so the mocks are live before any child asks the engine.
	installMockBackend(untrack(() => env));
</script>

{@render children()}
