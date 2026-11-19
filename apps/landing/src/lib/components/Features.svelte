<script lang="ts">
	// Order is the order of a real pipeline, and the accents match the node
	// categories in the app, so a colour learned here still means the same thing
	// on the canvas.
	const stages = [
		{
			title: 'Inputs',
			lead: 'Anything the machine can hear becomes a source node.',
			dot: 'bg-emerald-500',
			active: 'border-emerald-500/50 mix-bg-emerald-500 text-emerald-400',
			items: [
				'Microphone & audio interfaces',
				'System audio',
				'Per-app audio',
				'WAV file playback with looping',
				'Virtual device loopback'
			]
		},
		{
			title: 'Effects',
			lead: 'Real-time processing, every parameter live.',
			dot: 'bg-violet-500',
			active: 'border-violet-500/50 mix-bg-violet-500 text-violet-400',
			items: [
				'10-band graphic EQ, 32 Hz to 16 kHz',
				'Compressor with soft knee & sidechain',
				'Look-ahead brick-wall limiter',
				'Noise gate with hold & sidechain',
				'DeepFilterNet noise suppression',
				'Freeverb reverb & stereo delay',
				'Gain, channel balance, saturation',
				'Mute & per-node bypass'
			]
		},
		{
			title: 'Plugins',
			lead: 'Your own plugin library, hosted as nodes.',
			dot: 'bg-fuchsia-500',
			active: 'border-fuchsia-500/50 mix-bg-fuchsia-500 text-fuchsia-400',
			items: [
				'Host third-party CLAP, AU & VST3 plugins as nodes',
				'Plugin editor opens from the node',
				'Parameters exposed & automatable on the node',
				'Full node width when the plugin takes it, stereo pairs when not',
				'Broad plugin format support'
			]
		},
		{
			title: 'Monitoring',
			lead: 'Measure the signal at any point in the chain.',
			dot: 'bg-amber-500',
			active: 'border-amber-500/50 mix-bg-amber-500 text-amber-400',
			items: [
				'Peak & RMS level meter',
				'EBU R128 loudness (M/S/I LUFS)',
				'True Peak, LRA, PLR, dynamic range',
				'Live waveform envelope',
				'Meters anywhere in the chain'
			]
		},
		{
			title: 'Outputs',
			lead: 'Send the result to hardware, disk, or another app.',
			dot: 'bg-sky-500',
			active: 'border-sky-500/50 mix-bg-sky-500 text-sky-400',
			items: [
				'Audio interfaces',
				'WAV, FLAC, AIFF recording',
				'Opus, MP3, AAC (macOS) export',
				'Virtual audio devices',
				'DAW loopback feed'
			]
		},
		{
			title: 'Network',
			lead: 'The graph does not have to end on this machine.',
			dot: 'bg-rose-500',
			active: 'border-rose-500/50 mix-bg-rose-500 text-rose-400',
			items: [
				'WebRTC collaboration with peers',
				'UDP send & receive over direct IP',
				'Opus or raw PCM on the wire',
				'Per-peer routing back into the graph'
			]
		}
	];

	let selected = $state(0);
	const stage = $derived(stages[selected]);
</script>

<section class="mx-auto px-6 pb-24 xl:w-4xl">
	<div class="mb-12 text-center">
		<h2 class="text-3xl font-bold tracking-tight">
			One signal, end to end &mdash; capture, effects, output
		</h2>
		<p class="mx-auto mt-3 max-w-xl text-pretty text-neutral-1000">
			From microphone and system audio capture through noise suppression, EQ and compression to
			recording &mdash; arranged the way audio actually travels. Pick a stage.
		</p>
	</div>

	<div class="relative">
		<div class="rail absolute inset-x-4 top-1/2 hidden h-px -translate-y-1/2 md:block"></div>
		<div class="relative flex flex-wrap justify-center gap-2 md:justify-between">
			{#each stages as s, i (s.title)}
				<button
					type="button"
					onclick={() => (selected = i)}
					aria-pressed={selected === i}
					class={[
						'inline-flex items-center gap-2 rounded-full border px-4 py-2 text-sm font-semibold transition-all duration-300',
						selected === i
							? s.active
							: 'border-neutral-300 bg-neutral-100 text-neutral-1000 hover:border-neutral-500'
					]}
				>
					<span class={['size-2 rounded-full', s.dot]}></span>
					{s.title}
				</button>
			{/each}
		</div>
	</div>

	<div class="card mt-8">
		<h3 class="text-lg font-semibold tracking-tight">{stage.lead}</h3>
		<ul class="mt-5 flex flex-wrap gap-2">
			{#each stage.items as item (item)}
				<li
					class="rounded-xl border border-neutral-300 bg-neutral-200 px-3 py-1.5 text-sm text-neutral-1100"
				>
					{item}
				</li>
			{/each}
		</ul>
	</div>
</section>

<style>
	/* The rail is the hero's cable, flattened: same 5/5 dash, same 0.5s crawl. */
	.rail {
		background-image: repeating-linear-gradient(
			to right,
			var(--color-neutral-500) 0 5px,
			transparent 5px 10px
		);
		animation: railcrawl 0.5s linear infinite;
	}

	@keyframes railcrawl {
		to {
			background-position-x: 10px;
		}
	}

	@media (prefers-reduced-motion: reduce) {
		.rail {
			animation: none;
		}
	}
</style>
