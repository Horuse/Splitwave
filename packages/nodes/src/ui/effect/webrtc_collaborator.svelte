<script lang="ts">
	import { Button, PasswordInput, Segmented, SignalBars } from '@splitwave/ui';
	import { onDestroy, onMount, untrack } from 'svelte';
	import { useNodeConnections, useSvelteFlow, Position, type Node, type NodeProps } from '@xyflow/svelte';
	import Handle from '../_handle.svelte';
	import type { WebRtcCollaboratorNodeData } from '@splitwave/nodes';
	import type { OpusApplication, NetCodec } from '@splitwave/nodes';
	import { LossWindow } from '@splitwave/nodes';
	import Wrapper from '../node.svelte';
	import PeopleTeam from '~icons/fluent/people-team-16-filled';
	import Copy from '~icons/fluent/copy-16-regular';
	import Checkmark from '~icons/fluent/checkmark-16-filled';
	import Speaker from '~icons/fluent/speaker-2-16-filled';
	import SpeakerMute from '~icons/fluent/speaker-mute-16-filled';
	import Dismiss from '~icons/fluent/dismiss-16-filled';
	import { CAPTION, FIELD, MAIN_HANDLE, channelColor, channelLabel, channelOf, handleEdgeStyle, parsePort, peerHandle } from '../utils';
	import { formatHz } from '@splitwave/nodes';
	import { getEnginePort } from '../../engine';

	const engine = getEnginePort();

	type WebRtcNodeType = Node<WebRtcCollaboratorNodeData, 'webRtcCollaborator'>;
	let { id, data }: NodeProps<WebRtcNodeType> = $props();

	const flow = useSvelteFlow();

	let srcTooltip = $derived.by(() => {
		if (engine.sampleRate === 48_000) return undefined;
		return `WebRTC audio operates at ${formatHz(48_000)} (resampled from ${formatHz(engine.sampleRate)} and back)`;
	});

	const MAX_CHANNELS = 255;
	const wired = useNodeConnections(() => ({ id, handleType: 'target' }));
	let channelCount = $derived(
		Math.max(
			1,
			wired.current.reduce((n, c) => {
				const ch = c.targetHandle ? channelOf(c.targetHandle) : null;
				return ch === null ? n : Math.max(n, ch);
			}, 0)
		)
	);

	// Peers are told how many channels to expect, so it tracks the cables.
	$effect(() => {
		const next = Math.min(channelCount, MAX_CHANNELS);
		if (next === untrack(() => data.channels)) return;
		flow.updateNodeData(id, { channels: next });
		pushIdentity(untrack(() => data.name) ?? '', next);
	});

	let roomCode = $state('');
	let joinInput = $state('');
	let password = $state('');
	let joinPassword = $state('');
	let phase = $state<'idle' | 'hosting' | 'joining'>('idle');
	let busy = $state(false);
	let error = $state('');
	let peers = $state<{ peerId: string; muted: boolean; name: string; channels: number[] }[]>([]);
	let quality = $state<Record<string, { ping: number; loss: number }>>({});
	// Jitter buffer depth: latency this node adds, separate from the link's RTT.
	let bufferMs = $state(0);
	const lossWindows = new Map<string, LossWindow>();
	let copied = $state(false);

	function removePeerEdges(peerId: string) {
		const orphaned = flow
			.getEdges()
			.filter((e) => {
				const p = parsePort(e.sourceHandle);
				return e.source === id && p?.signal === 'audio' && p.port.kind === 'peer' && p.port.peer === peerId;
			})
			.map((e) => ({ id: e.id }));
		if (orphaned.length > 0) flow.deleteElements({ edges: orphaned });
	}

	function pushIdentity(name: string, channels: number) {
		engine.audio.webrtcSetIdentity(id, name, channels, data.codec ?? 'opus', data.opusBitrate, data.opusApplication).catch(() => {});
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

	const CODECS: { value: NetCodec; label: string; sub: string }[] = [
		{ value: 'opus', label: 'Opus', sub: 'compressed' },
		{ value: 'pcm-f32', label: 'PCM', sub: 'f32' },
		{ value: 'pcm-i16', label: 'PCM', sub: 'i16' }
	];

	function setCodec(codec: NetCodec) {
		flow.updateNodeData(id, { codec });
		pushIdentity(data.name ?? '', channelCount);
	}

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
			const hash = await engine.audio.webrtcHashPassword(password);
			roomCode = await engine.audio.webrtcCreateRoom(id, data.opusBitrate, data.opusApplication, hash);
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
			const hash = await engine.audio.webrtcHashPassword(joinPassword);
			await engine.audio.webrtcJoinRoom(id, joinInput.trim().toUpperCase(), data.opusBitrate, data.opusApplication, hash);
			phase = 'joining';
		} catch (e) {
			error = String(e);
			busy = false;
		}
	}

	async function confirmAction(message: string, confirmLabel: string): Promise<boolean> {
		const ok = await engine.confirm('', {
			message,
			confirmLabel,
			danger: true,
			canClose: true
		});
		return ok === true;
	}

	async function destroy() {
		if (!(await confirmAction('Destroy this room? All participants will be disconnected.', 'Destroy'))) return;
		await cancel();
	}

	async function leave() {
		if (!(await confirmAction('Leave this room?', 'Leave'))) return;
		await cancel();
	}

	async function cancel() {
		await engine.audio.webrtcLeaveRoom(id).catch(() => {});
		for (const p of peers) removePeerEdges(p.peerId);
		stopPingPolling();
		phase = 'idle';
		roomCode = '';
		joinInput = '';
		password = '';
		joinPassword = '';
		peers = [];
		quality = {};
		lossWindows.clear();
		error = '';
		busy = false;
	}

	async function toggleMute(peerId: string, current: boolean) {
		const muted = !current;
		await engine.audio.webrtcSetPeerMuted(id, peerId, muted).catch(() => {});
		peers = peers.map((p) => (p.peerId === peerId ? { ...p, muted } : p));
	}

	async function disconnect(peerId: string) {
		await engine.audio.webrtcDisconnectPeer(id, peerId).catch(() => {});
		peers = peers.filter((p) => p.peerId !== peerId);
		removePeerEdges(peerId);
	}

	let pingInterval: ReturnType<typeof setInterval> | null = null;

	function startPingPolling() {
		if (pingInterval) return;
		pingInterval = setInterval(async () => {
			if (peers.length === 0) return;
			bufferMs = await engine.audio.webrtcBufferMs(id).catch(() => 0);
			const raw = await engine.audio.webrtcPeerStats(id).catch(() => ({}));
			const next: Record<string, { ping: number; loss: number }> = {};
			for (const [peerId, s] of Object.entries(raw)) {
				let w = lossWindows.get(peerId);
				if (!w) {
					w = new LossWindow();
					lossWindows.set(peerId, w);
				}
				next[peerId] = { ping: s.pingMs, loss: w.update(s.packets, s.lost) };
			}
			quality = next;
		}, 2000);
	}

	function stopPingPolling() {
		if (pingInterval) {
			clearInterval(pingInterval);
			pingInterval = null;
		}
		bufferMs = 0;
	}

	const unlistens = Promise.all([
		engine.audio.onWebrtcConnected((e) => {
			if (e.nodeId !== id) return;
			busy = false;
			if (!peers.find((p) => p.peerId === e.peerId)) peers = [...peers, { peerId: e.peerId, muted: false, name: '', channels: [] }];
			startPingPolling();
		}),
		engine.audio.onWebrtcDisconnected((e) => {
			if (e.nodeId !== id) return;
			peers = peers.filter((p) => p.peerId !== e.peerId);
			removePeerEdges(e.peerId);
			// A guest whose host is gone returns to idle.
			if (phase === 'joining' && peers.length === 0) {
				stopPingPolling();
				phase = 'idle';
				error = 'Host left the room';
			}
		}),
		engine.audio.onWebrtcError((e) => {
			if (e.nodeId !== id) return;
			error = e.error;
			busy = false;
			phase = 'idle';
		}),
		engine.audio.onWebrtcMeta((e) => {
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
		const state = await engine.audio.webrtcSessionState(id).catch(() => null);
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

<Wrapper label="WebRTC" icon={PeopleTeam} accent="network" {srcTooltip} hasInput channelIo nodeId={id} maxChannels={MAX_CHANNELS}>
	<div class="nodrag nopan flex w-52 flex-col gap-2">
		<div class="flex flex-col gap-1">
			<span class={CAPTION}>Your name</span>
			<input class={FIELD} placeholder="This device" value={data.name ?? ''} onchange={(e) => setName(e.currentTarget.value)} />
		</div>

		<div class="flex flex-col gap-1">
			<span class={CAPTION}>Codec</span>
			<Segmented
				class="nodrag nopan"
				options={CODECS.map((c) => ({ value: c.value, label: c.label, subtitle: c.sub }))}
				value={data.codec}
				onSelect={setCodec} />
		</div>

		{#if (data.codec ?? 'opus') === 'opus'}
			<div class="flex flex-col gap-1">
				<span class={CAPTION}>Bitrate (kbps)</span>
				<Segmented
					class="nodrag nopan"
					options={BITRATES.map((b) => ({ value: b.bps, label: b.label }))}
					value={data.opusBitrate}
					onSelect={setBitrate} />
			</div>
			<div class="flex flex-col gap-1">
				<span class={CAPTION}>Mode</span>
				<Segmented
					class="nodrag nopan"
					options={APPS.map((a) => ({ value: a.value, label: a.label, subtitle: a.sub }))}
					value={data.opusApplication}
					onSelect={setApp} />
			</div>
		{/if}

		<div class="border-t border-theme/5"></div>

		{#if phase === 'idle'}
			<!-- Host a room, or join one by code. -->
			<div class="flex flex-col gap-1">
				<span class={CAPTION}>Host a room</span>
				<PasswordInput class="nodrag nopan" bind:value={password} placeholder="Password (optional)" />
				<Button size="sm" class="nodrag nopan" disabled={busy} onclick={createRoom}>{busy ? 'Creating...' : 'Create room'}</Button>
			</div>
			<div class="flex items-center gap-2 text-[10px] text-neutral-600">
				<div class="flex-1 border-t border-theme/5"></div>
				or
				<div class="flex-1 border-t border-theme/5"></div>
			</div>
			<div class="flex flex-col gap-1">
				<span class={CAPTION}>Join by code</span>
				<input class={[FIELD, 'tracking-widest uppercase']} placeholder="ABC123" maxlength={6} bind:value={joinInput} />
				<div class="flex gap-1">
					<PasswordInput class="nodrag nopan" bind:value={joinPassword} placeholder="Password" />
					<Button size="sm" class="nodrag nopan" disabled={busy || joinInput.trim().length < 6} onclick={joinRoom}>Join</Button>
				</div>
			</div>
		{:else}
			{#if phase === 'hosting'}
				<!-- The code to share, copyable in one click. -->
				<div class="flex items-center gap-2 rounded-lg border border-theme/5 bg-(--field-bg,var(--lv-1)) py-1.5 pr-1.5 pl-3">
					<div class="flex flex-1 flex-col gap-0.5">
						<span class="text-[9px] text-neutral-600">Room code</span>
						<span class="font-mono text-lg leading-none font-semibold tracking-[0.25em] text-neutral-1100">{roomCode}</span>
					</div>
					<Button size="sm" icon class="nodrag nopan" title="Copy room code" onclick={() => copy(roomCode)}>
						{#if copied}<Checkmark class="size-3.5" />{:else}<Copy class="size-3.5" />{/if}
					</Button>
				</div>
			{/if}
			<div class="flex items-center justify-between gap-2">
				<span
					class={[
						'flex items-center gap-1.5 rounded-md px-1.5 py-0.5 text-[10px]',
						peers.length > 0 ? 'bg-emerald-500/15 text-emerald-500' : 'bg-amber-500/15 text-amber-500'
					]}>
					<span class={['size-1.5 rounded-full', peers.length > 0 ? 'bg-emerald-500' : 'bg-amber-500']}></span>
					{peers.length > 0 ? `${peers.length} connected` : phase === 'hosting' ? 'Waiting for peer' : 'Connecting'}
				</span>
				{#if phase === 'hosting'}
					<Button tone="danger" size="sm" class="nodrag nopan" onclick={destroy}>Close room</Button>
				{:else if peers.length === 0}
					<Button tone="danger" size="sm" class="nodrag nopan" onclick={cancel}>Cancel</Button>
				{:else}
					<Button tone="danger" size="sm" class="nodrag nopan" onclick={leave}>Leave</Button>
				{/if}
			</div>
		{/if}

		{#if error}
			<p class="text-[10px] text-red-500">{error}</p>
		{/if}

		{#if phase !== 'idle' || peers.length > 0}
			<div class="flex flex-col">
				{#if peers.length > 0}
					<!-- One buffer serves every peer, unlike the per-peer ping. -->
					<div class="flex items-center justify-between py-1 font-mono text-[10px]">
						<span class="text-neutral-700">Added delay</span>
						<span class="text-neutral-1000 tabular-nums">{bufferMs ? `${bufferMs} ms` : '--'}</span>
					</div>
				{/if}
				{#if phase !== 'idle'}
					<div class="relative flex min-h-6 items-center justify-between gap-1 border-t border-theme/5 py-1">
						<span class="text-[10px] text-neutral-900">All peers</span>
						<span class="mr-2 text-[9px] text-neutral-600">mixed</span>
						<Handle type="source" id={MAIN_HANDLE} position={Position.Right} class="handle" style={handleEdgeStyle('#a3a3a3', 'source')} />
					</div>
				{/if}
				{#each peers as peer (peer.peerId)}
					<!-- Peer header and this peer's full mix. -->
					<div class="relative flex min-h-7 items-center gap-1.5 border-t border-theme/5 py-1">
						<span
							class="flex size-5 shrink-0 items-center justify-center rounded-full bg-(--lv-2) text-[10px] font-semibold text-neutral-1000 uppercase">
							{(peer.name || peer.peerId).slice(0, 1)}
						</span>
						<span class="min-w-0 flex-1 truncate text-[11px] text-neutral-1000">{peer.name || peer.peerId.slice(0, 10)}</span>
						<span class="flex shrink-0 items-center gap-1 font-mono text-[9px] text-neutral-600 tabular-nums">
							<SignalBars loss={quality[peer.peerId]?.loss ?? null} ping={quality[peer.peerId]?.ping ?? null} />
							{quality[peer.peerId]?.ping ? `${quality[peer.peerId].ping} ms` : '--'}
						</span>
						<Button
							size="xs"
							icon
							class="nodrag nopan"
							active={!peer.muted}
							title={peer.muted ? 'Unmute peer' : 'Mute peer'}
							onclick={() => toggleMute(peer.peerId, peer.muted)}>
							{#if peer.muted}<SpeakerMute class="size-3" />{:else}<Speaker class="size-3" />{/if}
						</Button>
						<Button size="xs" icon tone="danger" class="nodrag nopan" title="Disconnect peer" onclick={() => disconnect(peer.peerId)}>
							<Dismiss class="size-3" />
						</Button>
						<Handle
							type="source"
							id={peerHandle(peer.peerId)}
							position={Position.Right}
							class="handle"
							style={handleEdgeStyle('#a3a3a3', 'source')} />
					</div>
					<!-- Per-channel outputs for this peer. -->
					{#each peer.channels as c (c)}
						<div class="relative flex min-h-5 items-center justify-end gap-1 pr-2">
							<span class="shrink-0 font-mono text-[9px]" style="color:{channelColor(c)}">
								{channelLabel(c, peer.channels.length)}
							</span>
							<Handle
								type="source"
								id={peerHandle(peer.peerId, c)}
								position={Position.Right}
								class="handle"
								style={handleEdgeStyle(channelColor(c), 'source')} />
						</div>
					{/each}
				{/each}
			</div>
		{/if}
	</div>
</Wrapper>
