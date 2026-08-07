<script lang="ts">
	import { Refresh } from '$lib/components/icons';
	import ComboboxAction from './combobox_action.svelte';

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

<ComboboxAction {label} icon={Refresh} iconClass={spinning ? 'animate-spin' : ''} onclick={run} />
