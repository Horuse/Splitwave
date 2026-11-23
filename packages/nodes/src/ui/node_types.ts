import type { Component } from 'svelte';
import type { NodeKind } from '../index';
import Microphone from './input/microphone.svelte';
import SystemAudio from './input/system_audio.svelte';
import AppAudio from './input/app_audio.svelte';
import AudioFile from './input/audio_file.svelte';
import Speaker from './output/speaker.svelte';
import FileRecording from './output/file_recording.svelte';
import Gain from './effect/gain.svelte';
import Mute from './effect/mute.svelte';
import ChannelBalance from './effect/channel_balance.svelte';
import Saturator from './effect/saturator.svelte';
import Eq from './effect/eq.svelte';
import LevelMeter from './effect/level_meter.svelte';
import LufsMeter from './effect/lufs_meter.svelte';
import Waveform from './effect/waveform.svelte';
import Spectrum from './effect/spectrum.svelte';
import Limiter from './effect/limiter.svelte';
import Compressor from './effect/compressor.svelte';
import NoiseGate from './effect/noise_gate.svelte';
import Delay from './effect/delay.svelte';
import Reverb from './effect/reverb.svelte';
import NoiseSuppressor from './effect/noise_suppressor.svelte';
import Declick from './effect/declick.svelte';
import DeEsser from './effect/de_esser.svelte';
import Plugin from './effect/plugin.svelte';
import WebRtcCollaborator from './effect/webrtc_collaborator.svelte';
import NetReceiver from './input/net_receiver.svelte';
import NetSender from './output/net_sender.svelte';
import KeyInput from './control/key_input.svelte';
import Toggle from './control/toggle.svelte';
import Logic from './control/logic.svelte';
import Cycle from './control/cycle.svelte';
import Timer from './control/timer.svelte';
import If from './control/if.svelte';
import Switch from './control/switch.svelte';
import Select from './control/select.svelte';
import LevelTrigger from './control/level_trigger.svelte';

/** Typed loosely so a story can render a node outside a flow; still a valid xyflow NodeTypes. */
export const nodeTypes: Record<NodeKind, Component<any>> = {
	microphone: Microphone,
	systemAudio: SystemAudio,
	appAudio: AppAudio,
	audioFile: AudioFile,
	speaker: Speaker,
	fileRecording: FileRecording,
	gain: Gain,
	mute: Mute,
	channelBalance: ChannelBalance,
	saturator: Saturator,
	eq: Eq,
	levelMeter: LevelMeter,
	lufsMeter: LufsMeter,
	waveform: Waveform,
	spectrum: Spectrum,
	limiter: Limiter,
	compressor: Compressor,
	noiseGate: NoiseGate,
	delay: Delay,
	reverb: Reverb,
	noiseSuppressor: NoiseSuppressor,
	declick: Declick,
	deEsser: DeEsser,
	plugin: Plugin,
	webRtcCollaborator: WebRtcCollaborator,
	netReceiver: NetReceiver,
	netSender: NetSender,
	keyInput: KeyInput,
	toggle: Toggle,
	cycle: Cycle,
	timer: Timer,
	logic: Logic,
	if: If,
	switch: Switch,
	select: Select,
	levelTrigger: LevelTrigger
};
