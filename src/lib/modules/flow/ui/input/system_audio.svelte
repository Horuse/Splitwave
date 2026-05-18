<script lang="ts">
	import { onMount } from 'svelte';
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import { openUrl } from '@tauri-apps/plugin-opener';
	import type { SystemAudioNodeData } from '$lib/modules/pipeline/types';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import type { PermissionState } from '$lib/modules/audio/types';
	import Wrapper from '../node.svelte';
	import InputMeter from './_input_meter.svelte';
	import Slider from '../effect/_slider.svelte';

	type SystemAudioNodeType = Node<SystemAudioNodeData, 'systemAudio'>;
	let { id, data }: NodeProps<SystemAudioNodeType> = $props();

	const flow = useSvelteFlow();

	let permission = $state<PermissionState | null>(null);
	let checking = $state(false);

	function onToggle(e: Event) {
		const checked = (e.currentTarget as HTMLInputElement).checked;
		flow.updateNodeData(id, { excludeCurrentApp: checked });
	}

	async function refreshPermission() {
		checking = true;
		try {
			permission = await audioMethods.checkScreenRecordingPermission();
		} catch {
			permission = 'unknown';
		} finally {
			checking = false;
		}
	}

	onMount(refreshPermission);

	async function openPrivacySettings() {
		try {
			await openUrl('x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture');
		} catch {
			// fall through silently -- not all hosts support deep links
		}
	}

	function setVolume(pct: number) {
		const scalar = Math.max(0, Math.min(1, pct / 100));
		flow.updateNodeData(id, { volume: scalar });
		audioMethods.setInputVolume(id, scalar).catch(() => {});
	}

	function formatPct(p: number): string {
		return `${Math.round(p)}%`;
	}

	let volumePct = $derived((data.volume ?? 1) * 100);
</script>

<Wrapper label="System Audio" accent="input" hasOutput>
	<div class="flex w-64 flex-col gap-3">
		<p class="text-[11px] text-neutral-900">
			Captures all system output via ScreenCaptureKit (macOS 13+).
		</p>
		{#if permission !== 'allowed'}
			<div class={[
				'flex items-center justify-between gap-2 rounded border px-2 py-1 text-[10px]',
				permission === 'denied' && 'border-red-300 bg-red-50 text-red-700',
				(permission === 'unknown' || permission === null) && 'border-neutral-300 bg-neutral-100 text-neutral-1000'
			]}>
				<span class="flex items-center gap-1.5">
					<span
						class={[
							'inline-block h-2 w-2 rounded-full',
							permission === 'denied' && 'bg-red-500',
							(permission === 'unknown' || permission === null) && 'bg-neutral-500'
						]}
					></span>
					<span>
						{#if permission === 'denied'}
							Screen Recording denied
						{:else}
							Checking permission…
						{/if}
					</span>
				</span>
				{#if permission === 'denied'}
					<button
						type="button"
						class="nodrag nopan shrink-0 rounded border border-red-400 bg-red-100 px-1.5 py-0.5 hover:bg-red-200"
						onclick={openPrivacySettings}
					>
						Open Settings
					</button>
				{:else}
					<button
						type="button"
						class="nodrag nopan shrink-0 rounded border border-neutral-300 bg-neutral-100 px-1.5 py-0.5 hover:bg-neutral-200 disabled:opacity-50"
						title="Re-check"
						disabled={checking}
						onclick={refreshPermission}
					>
						⟳
					</button>
				{/if}
			</div>
		{/if}
		<label class="nodrag nopan flex items-center gap-2 text-xs text-neutral-1000">
			<input
				type="checkbox"
				class="nodrag nopan rounded"
				checked={data.excludeCurrentApp ?? true}
				onchange={onToggle}
			/>
			Exclude this app (avoid feedback)
		</label>
		<InputMeter nodeId={id} />
		<Slider
			label="Volume"
			value={volumePct}
			min={0}
			max={100}
			step={1}
			format={formatPct}
			defaultValue={100}
			ticks={[25, 50, 75]}
			onChange={setVolume}
		/>
	</div>
</Wrapper>
