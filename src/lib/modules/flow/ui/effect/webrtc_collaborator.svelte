<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import { useSvelteFlow, Handle, Position, type Node, type NodeProps } from '@xyflow/svelte';
	import type { WebRtcCollaboratorNodeData } from '$lib/modules/pipeline/types';
	import type { OpusApplication } from '$lib/modules/pipeline/types';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import Wrapper from '../node.svelte';

	type WebRtcNodeType = Node<WebRtcCollaboratorNodeData, 'webRtcCollaborator'>;
	let { id, data }: NodeProps<WebRtcNodeType> = $props();

	const flow = useSvelteFlow();

	const MAX_CHANNELS = 10;
	let channelCount = $derived(Math.min(Math.max(data.channels ?? 1, 1), MAX_CHANNELS));
	let inputs = $derived(
		Array.from({ length: channelCount }, (_, i) => ({ id: `ch${i + 1}`, label: `in ${i + 1}` }))
	);

	let roomCode = $state('');
	let joinInput = $state('');
	let phase = $state<'idle' | 'hosting' | 'joining'>('idle');
	let busy = $state(false);
	let error = $state('');
	let peers = $state<{ peerId: string; muted: boolean; name: string; channels: number[] }[]>([]);
	let pings = $state<Record<string, number>>({});
	let copied = $state(false);

	function removePeerEdges(peerId: string) {
		const mix = `peer:${peerId}`;
		const prefix = `peer:${peerId}:`;
		const orphaned = flow
			.getEdges()
			.filter(
				(e) =>
					e.source === id && (e.sourceHandle === mix || e.sourceHandle?.startsWith(prefix))
			)
			.map((e) => ({ id: e.id }));
		if (orphaned.length > 0) flow.deleteElements({ edges: orphaned });
	}

	function pushIdentity(name: string, channels: number) {
		audioMethods
			.webrtcSetIdentity(id, name, channels, data.opusBitrate, data.opusApplication)
			.catch(() => {});
	}

	function addChannel() {
		if (channelCount >= MAX_CHANNELS) return;
		const next = channelCount + 1;
		flow.updateNodeData(id, { channels: next });
		pushIdentity(data.name ?? '', next);
	}

	function removeChannel() {
		if (channelCount <= 1) return;
		const handle = `ch${channelCount}`;
		const orphaned = flow
			.getEdges()
			.filter((e) => e.target === id && e.targetHandle === handle)
			.map((e) => ({ id: e.id }));
		if (orphaned.length > 0) flow.deleteElements({ edges: orphaned });
		const next = channelCount - 1;
		flow.updateNodeData(id, { channels: next });
		pushIdentity(data.name ?? '', next);
	}

	function setName(name: string) {
		flow.updateNodeData(id, { name });
		pushIdentity(name, channelCount);
	}

	const BITRATES: { bps: number; label: string }[] = [
		{ bps: 32_000, label: '32' },
		{ bps: 64_000, label: '64' },
		{ bps: 96_000, label: '96' },
		{ bps: 128_000, label: '128' }
	];

	const APPS: { value: OpusApplication; label: string; sub: string }[] = [
		{ value: 'voip', label: 'VoIP', sub: 'voice' },
		{ value: 'audio', label: 'Audio', sub: 'music' },
		{ value: 'low-delay', label: 'Low', sub: 'delay' }
	];

	function setBitrate(bps: number) {
		flow.updateNodeData(id, { opusBitrate: bps });
	}

	function setApp(app: OpusApplication) {
		flow.updateNodeData(id, { opusApplication: app });
	}

	function copy(text: string) {
		navigator.clipboard.writeText(text).then(() => {
			copied = true;
			setTimeout(() => (copied = false), 1500);
		});
	}

	async function createRoom() {
		busy = true;
		error = '';
		try {
			roomCode = await audioMethods.webrtcCreateRoom(id, data.opusBitrate, data.opusApplication);
			phase = 'hosting';
		} catch (e) {
			error = String(e);
		} finally {
			busy = false;
		}
	}

	async function joinRoom() {
		busy = true;
		error = '';
		try {
			await audioMethods.webrtcJoinRoom(id, joinInput.trim().toUpperCase(), data.opusBitrate, data.opusApplication);
			phase = 'joining';
		} catch (e) {
			error = String(e);
			busy = false;
		}
	}

	async function cancel() {
		await audioMethods.webrtcLeaveRoom(id).catch(() => {});
		for (const p of peers) removePeerEdges(p.peerId);
		stopPingPolling();
		phase = 'idle';
		roomCode = '';
		joinInput = '';
		peers = [];
		pings = {};
		error = '';
		busy = false;
	}

	async function toggleMute(peerId: string, current: boolean) {
		const muted = !current;
		await audioMethods.webrtcSetPeerMuted(id, peerId, muted).catch(() => {});
		peers = peers.map((p) => (p.peerId === peerId ? { ...p, muted } : p));
	}

	async function disconnect(peerId: string) {
		await audioMethods.webrtcDisconnectPeer(id, peerId).catch(() => {});
		peers = peers.filter((p) => p.peerId !== peerId);
		removePeerEdges(peerId);
	}

	let pingInterval: ReturnType<typeof setInterval> | null = null;

	function startPingPolling() {
		if (pingInterval) return;
		pingInterval = setInterval(async () => {
			if (peers.length === 0) return;
			pings = await audioMethods.webrtcPeerPings(id).catch(() => ({}));
		}, 2000);
	}

	function stopPingPolling() {
		if (pingInterval) { clearInterval(pingInterval); pingInterval = null; }
	}

	const unlistens = Promise.all([
		audioMethods.onWebrtcConnected((e) => {
			if (e.nodeId !== id) return;
			busy = false;
			if (!peers.find((p) => p.peerId === e.peerId))
				peers = [...peers, { peerId: e.peerId, muted: false, name: '', channels: [] }];
			startPingPolling();
		}),
		audioMethods.onWebrtcDisconnected((e) => {
			if (e.nodeId !== id) return;
			peers = peers.filter((p) => p.peerId !== e.peerId);
			removePeerEdges(e.peerId);
		}),
		audioMethods.onWebrtcError((e) => {
			if (e.nodeId !== id) return;
			error = e.error;
			busy = false;
			phase = 'idle';
		}),
		audioMethods.onWebrtcMeta((e) => {
			if (e.nodeId !== id) return;
			const channels = Array.from({ length: e.channels }, (_, i) => i);
			peers = peers.map((p) => (p.peerId === e.peerId ? { ...p, name: e.name, channels } : p));
		})
	]);

	onDestroy(() => {
		stopPingPolling();
		unlistens.then((fns) => fns.forEach((f) => f()));
	});

	// The WebRTC session outlives this component, so restore UI state on remount.
	onMount(async () => {
		pushIdentity(data.name ?? '', channelCount);
		const state = await audioMethods.webrtcSessionState(id).catch(() => null);
		if (!state || state.phase === 'idle') return;
		phase = state.phase;
		roomCode = state.roomCode ?? '';
		peers = state.peers.map((p) => ({
			peerId: p.peerId,
			muted: p.muted,
			name: p.name ?? '',
			channels: p.channels ?? []
		}));
		if (peers.length > 0) startPingPolling();
	});
</script>

<Wrapper label="WebRTC" accent="effect">
	<div class="nodrag nopan flex w-52 flex-col gap-2">
		<!-- device / participant name -->
		<div class="flex flex-col gap-0.5">
			<span class="font-mono text-[9px] text-neutral-500">Name</span>
			<input
				class="nowheel h-6 rounded border border-neutral-300 bg-neutral-50 px-1.5 font-mono text-[10px] text-neutral-800 placeholder:text-neutral-400"
				placeholder="This device"
				value={data.name ?? ''}
				onchange={(e) => setName(e.currentTarget.value)}
			/>
		</div>

		<!-- inputs -->
		<div class="flex items-center justify-between">
			<span class="font-mono text-[9px] text-neutral-500">Inputs</span>
			<div class="flex items-center gap-1">
				<button
					type="button"
					class="flex h-4 w-4 items-center justify-center rounded border border-neutral-400 bg-neutral-100 font-mono text-[11px] leading-none text-neutral-800 hover:bg-neutral-200 disabled:opacity-40"
					disabled={channelCount <= 1}
					onclick={removeChannel}
				>
					-
				</button>
				<span class="w-4 text-center font-mono text-[10px] tabular-nums text-neutral-900">{channelCount}</span>
				<button
					type="button"
					class="flex h-4 w-4 items-center justify-center rounded border border-neutral-400 bg-neutral-100 font-mono text-[11px] leading-none text-neutral-800 hover:bg-neutral-200 disabled:opacity-40"
					disabled={channelCount >= MAX_CHANNELS}
					onclick={addChannel}
				>
					+
				</button>
			</div>
		</div>

		<!-- per-channel inputs (left handles) -->
		{#each inputs as ch (ch.id)}
			<div class="relative -ml-4 flex min-h-3 items-center gap-1 pl-4">
				<Handle type="target" id={ch.id} position={Position.Left} class="handle" />
				<span class="font-mono text-[9px] text-neutral-500">{ch.label}</span>
			</div>
		{/each}

		<!-- bitrate -->
		<div class="flex flex-col gap-0.5">
			<span class="font-mono text-[9px] text-neutral-500">Bitrate (kbps)</span>
			<div class="grid grid-cols-4 gap-[2px] rounded-sm border border-neutral-300 p-[2px]">
				{#each BITRATES as b (b.bps)}
					<button
						type="button"
						onclick={() => setBitrate(b.bps)}
						class={[
							'rounded-sm py-0.5 font-mono text-[10px] leading-none transition-colors',
							data.opusBitrate === b.bps
								? 'bg-neutral-900 text-white'
								: 'bg-neutral-100 text-neutral-900 hover:bg-neutral-200'
						]}
					>
						{b.label}
					</button>
				{/each}
			</div>
		</div>

		<!-- application -->
		<div class="flex flex-col gap-0.5">
			<span class="font-mono text-[9px] text-neutral-500">Mode</span>
			<div class="grid grid-cols-3 gap-[2px] rounded-sm border border-neutral-300 p-[2px]">
				{#each APPS as a (a.value)}
					<button
						type="button"
						onclick={() => setApp(a.value)}
						class={[
							'flex flex-col items-center rounded-sm py-0.5 leading-none transition-colors',
							data.opusApplication === a.value
								? 'bg-neutral-900 text-white'
								: 'bg-neutral-100 text-neutral-900 hover:bg-neutral-200'
						]}
					>
						<span class="font-mono text-[10px]">{a.label}</span>
						<span class="text-[8px] opacity-70">{a.sub}</span>
					</button>
				{/each}
			</div>
		</div>

		<hr class="border-neutral-300" />

		{#if phase === 'idle'}
			<button
				class="h-6 rounded border border-neutral-400 bg-neutral-100 font-mono text-[10px] text-neutral-800 hover:bg-neutral-200 disabled:opacity-50"
				disabled={busy}
				onclick={createRoom}
			>
				{busy ? 'creating…' : 'Create room'}
			</button>
			<div class="flex gap-1">
				<input
					class="nowheel h-6 min-w-0 flex-1 rounded border border-neutral-300 bg-neutral-50 px-1.5 font-mono text-[10px] uppercase text-neutral-800 placeholder:normal-case placeholder:text-neutral-400"
					placeholder="Room code"
					maxlength={6}
					bind:value={joinInput}
				/>
				<button
					class="h-6 rounded border border-neutral-400 bg-neutral-100 px-2 font-mono text-[10px] text-neutral-800 hover:bg-neutral-200 disabled:opacity-50"
					disabled={busy || joinInput.trim().length < 6}
					onclick={joinRoom}
				>
					Join
				</button>
			</div>
		{:else if phase === 'hosting'}
			<div class="flex items-center justify-between">
				<span class="font-mono text-[9px] text-neutral-500">Room code</span>
				<button
					class="h-4 rounded border border-neutral-300 bg-neutral-100 px-1.5 font-mono text-[9px] text-neutral-700 hover:bg-neutral-200"
					onclick={() => copy(roomCode)}>{copied ? 'copied!' : 'copy'}</button
				>
			</div>
			<div class="flex items-center justify-between rounded border border-neutral-300 bg-neutral-50 px-2 py-1">
				<span class="font-mono text-base font-bold tracking-widest text-neutral-900">{roomCode}</span>
			</div>
			<p class="font-mono text-[9px] text-neutral-500">Waiting for peer…</p>
			<button
				class="h-6 rounded border border-red-300 bg-red-50 font-mono text-[10px] text-red-700 hover:bg-red-100"
				onclick={cancel}
			>
				Cancel
			</button>
		{:else if phase === 'joining'}
			<p class="font-mono text-[9px] text-neutral-500">Connecting…</p>
			<button
				class="h-6 rounded border border-red-300 bg-red-50 font-mono text-[10px] text-red-700 hover:bg-red-100"
				onclick={cancel}
			>
				Cancel
			</button>
		{/if}

		{#if error}
			<p class="font-mono text-[9px] text-red-600">{error}</p>
		{/if}

		{#if phase !== 'idle' || peers.length > 0}
			<hr class="border-neutral-300" />
			{#if phase !== 'idle'}
				<div class="relative -mr-4 flex min-h-5 items-center justify-between gap-1 pr-4">
					<span class="truncate font-mono text-[9px] text-neutral-500">all peers</span>
					<span class="shrink-0 font-mono text-[9px] text-neutral-400">mixed</span>
					<Handle type="source" id="mixed" position={Position.Right} class="handle" />
				</div>
			{/if}
			{#each peers as peer (peer.peerId)}
				<!-- peer header + this peer's full mix -->
				<div class="relative -mr-4 flex min-h-5 items-center justify-between gap-1 pr-4">
					<span class="truncate font-mono text-[9px] text-neutral-700">
						{peer.name || peer.peerId.slice(0, 10)}
					</span>
					<span class="shrink-0 font-mono text-[9px] tabular-nums text-neutral-400">
						{pings[peer.peerId] ? `${pings[peer.peerId]}ms` : '--'}
					</span>
					<div class="flex gap-1">
						<button
							class={[
								'h-4 rounded border px-1 font-mono text-[9px]',
								peer.muted
									? 'border-amber-400 bg-amber-100 text-amber-800'
									: 'border-neutral-300 bg-neutral-100 text-neutral-700'
							]}
							onclick={() => toggleMute(peer.peerId, peer.muted)}
						>
							{peer.muted ? 'M' : 'ON'}
						</button>
						<button
							class="h-4 rounded border border-red-300 bg-red-50 px-1 font-mono text-[9px] text-red-700 hover:bg-red-100"
							onclick={() => disconnect(peer.peerId)}
						>
							x
						</button>
					</div>
					<Handle type="source" id={`peer:${peer.peerId}`} position={Position.Right} class="handle" />
				</div>
				<!-- per-channel outputs for this peer -->
				{#each peer.channels as c (c)}
					<div class="relative -mr-4 flex min-h-5 items-center justify-end gap-1 pr-4">
						<span class="shrink-0 font-mono text-[9px] text-neutral-400">in {c + 1}</span>
						<Handle
							type="source"
							id={`peer:${peer.peerId}:${c}`}
							position={Position.Right}
							class="handle"
						/>
					</div>
				{/each}
			{/each}
		{/if}
	</div>
</Wrapper>
