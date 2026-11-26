<script lang="ts">
	import { methods } from '$lib/modules/audio/methods';
	import type { VirtualDriverStatus } from '$lib/modules/audio/types';
	import { t } from '$lib/modules/i18n';

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
	<div class="warning-block flex flex-row items-end justify-between">
		<div class="flex flex-col gap-3">
			<span class="text-base font-bold">{t('audio.driverUpdateTitle')}</span>
			<p>{t('audio.driverUpdateMessage')}</p>
		</div>
		<button class="btn-warning h-full py-1.5" onclick={update} disabled={updating}>
			{updating ? t('common.updating') : t('common.update')}
		</button>
	</div>
{/if}
