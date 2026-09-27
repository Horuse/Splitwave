<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import NodeCanvas from '../harness/node_canvas.svelte';
	import { compose, dataArgs, envArgs, range, signalArgs, wiringArgs } from '../harness/args';
	import type { RecordingFormat } from '$lib/modules/pipeline/types';

	const FORMATS: Record<string, RecordingFormat> = {
		'wav f32': { kind: 'wav', bitDepth: 'f32' },
		'wav i24': { kind: 'wav', bitDepth: 'i24' },
		'wav i16': { kind: 'wav', bitDepth: 'i16' },
		'flac i24': { kind: 'flac', bitDepth: 'i24', compression: 'default' },
		'flac i16': { kind: 'flac', bitDepth: 'i16', compression: 'best' },
		'aiff i24': { kind: 'aiff', bitDepth: 'i24' },
		'opus 96k': { kind: 'opus', bitrate: 96_000, application: 'audio' },
		'mp3 192k': { kind: 'mp3', bitrateKbps: 192 },
		'aac 256k': { kind: 'aac', bitrate: 256_000 }
	};

	const { argTypes, args } = compose(
		dataArgs(
			{
				filePath: { control: 'select', options: ['/Users/demo/Recordings/take-01.wav', '/Users/demo/Recordings/take-01.flac'] },
				format: { control: 'select', options: Object.keys(FORMATS), mapping: FORMATS },
				mode: { control: 'inline-radio', options: ['new', 'overwrite', 'append'] },
				channels: { control: range(1, 16, 1) },
				sampleRate: { control: 'select', options: [44100, 48000, 88200, 96000] },
				waveformHidden: { control: 'boolean' }
			},
			{ filePath: '/Users/demo/Recordings/take-01.wav', format: 'wav f32', mode: 'new', channels: 2, sampleRate: 48000, waveformHidden: false }
		),
		wiringArgs(2),
		signalArgs(['signal', 'signalLevel', 'signalChannels', 'signalMotion']),
		envArgs(['platform', 'running', 'pipelineSampleRate'])
	);

	const { Story } = defineMeta({ title: 'Nodes/Outputs/File Recording', argTypes, args });
</script>

{#snippet template(a: Record<string, unknown>)}
	<NodeCanvas kind="fileRecording" args={a} />
{/snippet}

<Story name="Default" {template} />
<Story name="Recording" args={{ running: true }} {template} />
<Story name="No file" args={{ filePath: null }} {template} />
<Story name="FLAC" args={{ format: 'flac i24', filePath: '/Users/demo/Recordings/take-01.flac' }} {template} />
<Story name="Opus" args={{ format: 'opus 96k' }} {template} />
<Story name="MP3" args={{ format: 'mp3 192k' }} {template} />
<Story name="AAC" args={{ format: 'aac 256k' }} {template} />
<Story name="Append" args={{ mode: 'append' }} {template} />
<Story name="Overwrite" args={{ mode: 'overwrite' }} {template} />
<Story name="Mono" args={{ channels: 1, wiredInputs: 1 }} {template} />
<Story name="Multichannel" args={{ channels: 6, wiredInputs: 6, signalChannels: 6 }} {template} />
<Story name="Waveform hidden" args={{ waveformHidden: true }} {template} />
<Story name="Windows" args={{ platform: 'windows' }} {template} />
