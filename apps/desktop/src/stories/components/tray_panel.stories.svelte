<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { fn } from 'storybook/test';
	import { TrayPanel } from '$lib/modules/tray/ui';

	const HEADPHONES = {
		viewBox: '0 0 20 20',
		body: '<path fill="currentColor" d="M3 10a7 7 0 0 1 14 0v1h-3.5a.5.5 0 0 0-.5.5v6a.5.5 0 0 0 .5.5H16a2 2 0 0 0 2-2v-6a8 8 0 1 0-16 0v6a2 2 0 0 0 2 2h2.5a.5.5 0 0 0 .5-.5v-6a.5.5 0 0 0-.5-.5H3z"/>'
	};

	const PIPELINES = [
		{ id: 'music', name: 'Headphones - Music', icon: HEADPHONES, outputs: ['External Headphones'], stale: false },
		{ id: 'meeting', name: 'Meeting mix', icon: null, outputs: ['MacBook Pro Speakers', 'File Recording'], stale: false },
		{ id: 'guitar', name: 'Guitar monitoring', icon: null, outputs: [], stale: false }
	];

	const { Story } = defineMeta({
		title: 'Components/Tray Panel',
		component: TrayPanel,
		argTypes: {
			runningId: { control: 'select', options: [null, 'music', 'meeting', 'guitar'] },
			isRunning: { control: 'boolean' },
			busyId: { control: 'select', options: [null, 'music', 'meeting', 'guitar'] },
			error: { control: 'text' },
			waitingFor: { control: 'object' }
		},
		args: {
			pipelines: PIPELINES,
			runningId: null,
			isRunning: false,
			busyId: null,
			error: null,
			startedAt: Date.now() - 754_000,
			waitingFor: [],
			onrun: fn(),
			onstop: fn(),
			onopen: fn(),
			onquit: fn()
		}
	});
</script>

{#snippet template(args: Record<string, any>)}
	<div class="m-8 h-[420px] w-[320px]">
		<TrayPanel {...args as any} />
	</div>
{/snippet}

<Story name="Idle" {template} />
<Story name="Running" args={{ runningId: 'music', isRunning: true }} {template} />
<Story name="Waiting for device" args={{ runningId: 'music', isRunning: true, waitingFor: ['Fifine Microphone', 'Spotify'] }} {template} />
<Story name="Switching" args={{ runningId: 'music', isRunning: true, busyId: 'meeting' }} {template} />
<Story name="Error" args={{ error: 'audio device no longer available: External Headphones' }} {template} />
<Story name="Empty" args={{ pipelines: [] }} {template} />
<Story
	name="Overflow"
	args={{
		pipelines: [
			...Array.from({ length: 12 }, (_, i) => ({
				id: String(i),
				icon: i % 2 ? HEADPHONES : null,
				name: `Recording - interface, pedalboard and four microphones ${i + 1}`,
				outputs: ['Universal Audio Apollo Twin X Duo USB', 'BlackHole 16ch'],
				stale: false
			})),
			{ id: 'future', name: 'From the future', icon: null, outputs: [], stale: true }
		]
	}}
	{template} />
