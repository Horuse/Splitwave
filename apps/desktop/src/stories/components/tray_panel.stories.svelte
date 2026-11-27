<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { fn } from 'storybook/test';
	import { TrayPanel } from '$lib/modules/tray/ui';

	const PIPELINES = [
		{ id: 'music', name: 'Headphones - Music', outputs: ['External Headphones'], stale: false },
		{ id: 'meeting', name: 'Meeting mix', outputs: ['MacBook Pro Speakers', 'File Recording'], stale: false },
		{ id: 'guitar', name: 'Guitar monitoring', outputs: [], stale: false }
	];

	const { Story } = defineMeta({
		title: 'Components/Tray Panel',
		component: TrayPanel,
		argTypes: {
			runningId: { control: 'select', options: [null, 'music', 'meeting', 'guitar'] },
			isRunning: { control: 'boolean' },
			busyId: { control: 'select', options: [null, 'music', 'meeting', 'guitar'] },
			error: { control: 'text' }
		},
		args: {
			pipelines: PIPELINES,
			runningId: null,
			isRunning: false,
			busyId: null,
			error: null,
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
<Story name="Switching" args={{ runningId: 'music', isRunning: true, busyId: 'meeting' }} {template} />
<Story name="Error" args={{ error: 'audio device no longer available: External Headphones' }} {template} />
<Story name="Empty" args={{ pipelines: [] }} {template} />
<Story
	name="Overflow"
	args={{
		pipelines: [
			...Array.from({ length: 12 }, (_, i) => ({
				id: String(i),
				name: `Recording - interface, pedalboard and four microphones ${i + 1}`,
				outputs: ['Universal Audio Apollo Twin X Duo USB', 'BlackHole 16ch'],
				stale: false
			})),
			{ id: 'future', name: 'From the future', outputs: [], stale: true }
		]
	}}
	{template} />
