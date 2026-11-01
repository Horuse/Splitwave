<script lang="ts">
	import ControlSocket from '../_control_socket.svelte';

	interface Props {
		id: string;
		side: 'source' | 'target';
		label: string;
		high?: boolean;
	}
	let { id, side, label, high = false }: Props = $props();
</script>

<div
	class={[
		'relative flex min-h-4 items-center font-mono text-[9px] transition-colors',
		side === 'source' && 'justify-end',
		high ? 'text-yellow-600 dark:text-yellow-400' : 'text-neutral-900'
	]}>
	{label}
	<ControlSocket {id} {side} {high} />
</div>
