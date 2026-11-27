<script lang="ts">
	import type { ClassValue } from 'svelte/elements';
	import { Button } from '@splitwave/ui';
	import Play from '~icons/fluent/play-24-filled';
	import Stop from '~icons/fluent/stop-24-filled';
	import Switch from '~icons/fluent/arrow-swap-20-filled';

	/** `switch`: another pipeline is running and a click replaces it with this one. */
	export type RunState = 'run' | 'stop' | 'switch';

	let {
		state: runState,
		size,
		loading = false,
		disabled = false,
		class: className,
		onclick
	}: {
		state: RunState;
		size?: 'md' | 'lg' | 'xl';
		loading?: boolean;
		disabled?: boolean;
		class?: ClassValue;
		onclick: (event: MouseEvent) => void;
	} = $props();

	const STATES = {
		run: { label: 'Run', title: 'Run', tone: 'neutral', icon: Play },
		stop: { label: 'Stop', title: 'Stop', tone: 'danger', icon: Stop },
		switch: { label: 'Stop & Run', title: 'Stop the running pipeline and run this one', tone: 'warning', icon: Switch }
	} as const;

	let current = $derived(STATES[runState]);
</script>

<Button {size} {loading} {disabled} {onclick} class={className} tone={current.tone} loadingPosition="start" title={current.title}>
	<current.icon class="size-4" />
	{current.label}
</Button>
