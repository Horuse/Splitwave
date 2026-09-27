<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import MeterBar from '$lib/components/meter_bar.svelte';

	const GRADIENT = 'linear-gradient(to right, #22c55e 0%, #22c55e 70%, #eab308 70%, #eab308 90%, #f97316 90%, #f97316 95%, #ef4444 95%, #ef4444 100%)';
	const GRADIENT_V = GRADIENT.replace('to right', 'to top');

	const { Story } = defineMeta({
		title: 'Components/Meter Bar',
		component: MeterBar,
		argTypes: {
			pct: { control: { type: 'range', min: 0, max: 100, step: 1 } },
			hold: { control: { type: 'range', min: 0, max: 100, step: 1 } },
			orientation: { control: 'inline-radio', options: ['horizontal', 'vertical'] },
			ghost: { control: 'boolean' },
			hover: { control: 'boolean' }
		},
		args: {
			pct: 62,
			hold: 78,
			orientation: 'horizontal',
			ghost: false,
			hover: true,
			gradient: GRADIENT,
			hoverLabel: (p: number) => `${Math.round(p * 0.6 - 60)} dB`
		}
	});
</script>

{#snippet template(args: Record<string, any>)}
	<div class="p-8">
		<div class={args.orientation === 'vertical' ? 'h-40 w-3' : 'h-3 w-72'}>
			<MeterBar {...args} pct={args.pct} class="h-full w-full" gradient={args.orientation === 'vertical' ? GRADIENT_V : GRADIENT} />
		</div>
	</div>
{/snippet}

<Story name="Horizontal" {template} />
<Story name="Vertical" args={{ orientation: 'vertical' }} {template} />
<Story name="Near clip" args={{ pct: 98, hold: 100 }} {template} />
<Story name="Silent" args={{ pct: 0, hold: null }} {template} />
<Story name="Ghost" args={{ ghost: true }} {template} />
