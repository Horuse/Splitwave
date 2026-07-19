<script lang="ts">
	import { modalManager } from '../../modal';
	import Shell from './shell.svelte';
</script>

{#each modalManager.modals as entry (entry.id)}
	{@const SvelteComponent = entry.component}
	<Shell
		title={entry.title}
		description={entry.params.description as string | undefined}
		size={entry.params.size as 'sm' | 'md' | 'lg' | 'xl' | undefined}
		canClose={entry.params.canClose !== false}
		onClose={() => modalManager.close(entry.id)}
		zIndex={entry.zIndex}
	>
		<SvelteComponent modalId={entry.id} {...entry.params} />
	</Shell>
{/each}
