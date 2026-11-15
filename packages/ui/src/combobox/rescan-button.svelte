<script lang="ts">
	import IconRefresh from '~icons/fluent/arrow-clockwise-16-filled';
	import ComboboxAction from './combobox-action.svelte';

	interface Props {
		onRescan: () => void | Promise<void>;
		label?: string;
	}

	let { onRescan, label = 'Rescan' }: Props = $props();

	let spinning = $state(false);

	async function run() {
		if (spinning) return;
		spinning = true;
		try {
			await onRescan();
		} finally {
			spinning = false;
		}
	}
</script>

<ComboboxAction {label} icon={IconRefresh} iconClass={spinning ? 'animate-spin' : ''} onclick={run} />
