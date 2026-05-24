<script lang="ts">
	import { mockIPC } from '@tauri-apps/api/mocks';
	import '$lib/modules/theme/stores';
	import Grid from './_grid.svelte';

	const SPLITWAVE_DEVICE = { id: 'splitwave', name: 'Splitwave' };
	mockIPC((cmd) => {
		if (cmd === 'list_input_devices') return [{ ...SPLITWAVE_DEVICE, kind: 'input' }];
		if (cmd === 'list_output_devices') return [{ ...SPLITWAVE_DEVICE, kind: 'output' }];
		if (cmd === 'device_info') return { sampleRate: 48000, channels: 2, sampleFormat: 'f32' };
		if (cmd === 'get_device_volume') return 0.75;
		if (cmd.startsWith('list_')) return [];
		if (cmd === 'get_app_icons') return {};
		if (cmd === 'is_pipeline_running') return false;
		if (cmd === 'plugin:store|entries') return [];
		if (cmd === 'plugin:store|get') return null;
		return undefined;
	}, { shouldMockEvents: true });
</script>

<svelte:head>
	<style>
		:root { color-scheme: light; }
		html.dark { color-scheme: dark; }
		html, body { overflow: auto !important; }
	</style>
</svelte:head>

<Grid />
