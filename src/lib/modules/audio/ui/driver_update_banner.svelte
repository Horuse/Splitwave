<script lang="ts">
	import { methods } from '$lib/modules/audio/methods';
	import type { VirtualDriverStatus } from '$lib/modules/audio/types';

	let { onUpdated }: { onUpdated?: (status: VirtualDriverStatus) => void } = $props();

	let status = $state<VirtualDriverStatus | null>(null);
	let updating = $state(false);

	async function load() {
		try {
			status = await methods.virtualDriverStatus();
		} catch {
			status = null;
		}
	}

	async function update() {
		if (updating) return;
		updating = true;
		try {
			await methods.installVirtualDriver();
			await load();
			if (status) onUpdated?.(status);
		} catch {
			// keep banner on failure so user can retry
		} finally {
			updating = false;
		}
	}

	load();
</script>

{#if status?.needsUpdate}
	<div class="warning-block flex flex-row justify-between items-end">
		<div class="flex flex-col gap-3">
			<span class="text-base font-bold">Driver update required</span>
			<p>
				An older audio driver is installed and may cause audio glitches. Reinstall to update
				(requires your password, then restarts the audio service). Afterwards, go to Virtual
				devices and press Apply to recreate your devices.
			</p>
		</div>
		<button class="btn-warning h-full py-1.5" onclick={update} disabled={updating}>
			{updating ? 'Updating...' : 'Update'}
		</button>
	</div>
{/if}
