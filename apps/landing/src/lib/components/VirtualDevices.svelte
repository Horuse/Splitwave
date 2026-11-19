<script lang="ts">
	import WireDown from '#lib/components/WireDown.svelte';

	// Limits are the driver's, not marketing: `channels.clamp(1, 256)` in both
	// audio/virtual_device/macos.rs and linux.rs.
	const specs = [
		{ value: 'Up to 256', label: 'channels per device' },
		{ value: 'Any number', label: 'of devices, named by you' },
		{ value: 'No routing app', label: 'to install alongside' }
	];
</script>

<section class="mx-auto max-w-5xl px-6 py-20">
	<div class="mb-12 text-center">
		<h2 class="text-3xl font-bold tracking-tight">
			Virtual audio devices the whole system can see
		</h2>
		<p class="mx-auto mt-3 max-w-xl text-pretty text-neutral-1000">
			On macOS and Linux, Splitwave installs a driver and creates virtual audio cables &mdash;
			devices the system treats as real hardware. On Windows, Splitwave can download and set up
			VB-CABLE for you. Any app can play into one, and any app can pick one as its microphone. Both
			ends land in your graph.
		</p>
	</div>

	{#snippet box(title: string, sub: string, accent = false)}
		<div
			class={[
				'w-full rounded-2xl border bg-neutral-100 px-5 py-4 text-center',
				accent ? 'border-blue-500/35' : 'border-neutral-300'
			]}
		>
			<p class="font-semibold">{title}</p>
			<p class="mt-1 text-sm text-neutral-1000">{sub}</p>
		</div>
	{/snippet}

	<!-- SVG text does not reflow, so narrow screens get the same diagram as a
	     stacked column of real HTML instead of unreadable shrunken labels. -->
	<div class="flex flex-col items-center md:hidden">
		{@render box('Zoom, Chrome, a DAW', 'plays into it')}
		<WireDown color="#10b981" />
		{@render box('Virtual device', 'real hardware to the OS', true)}
		<WireDown color="#10b981" />
		{@render box('Your graph', 'arrives as a source node')}

		<div class="my-10 h-px w-16 bg-neutral-300"></div>

		{@render box('Your graph', 'processed and mixed')}
		<WireDown color="#0ea5e9" />
		{@render box('Virtual device', 'real hardware to the OS', true)}
		<WireDown color="#0ea5e9" />
		{@render box('Any app', 'picks it as its microphone')}
	</div>

	<svg
		viewBox="0 0 880 320"
		class="hidden w-full text-theme md:mx-auto md:block md:w-2xl lg:w-4xl"
		role="img"
		aria-label="Apps play into a Splitwave virtual device which feeds your graph, and your graph feeds a virtual device that apps pick as their microphone"
	>
		<defs>
			<marker
				id="vd-arrow-in"
				viewBox="0 0 8 8"
				refX="7"
				refY="4"
				markerWidth="6"
				markerHeight="6"
				orient="auto"
			>
				<path d="M0 0 L8 4 L0 8 z" fill="#10b981" />
			</marker>
			<marker
				id="vd-arrow-out"
				viewBox="0 0 8 8"
				refX="7"
				refY="4"
				markerWidth="6"
				markerHeight="6"
				orient="auto"
			>
				<path d="M0 0 L8 4 L0 8 z" fill="#0ea5e9" />
			</marker>
		</defs>

		<rect x="325" y="34" width="230" height="252" rx="18" class="diagram-box" stroke-width="1" />
		<rect
			x="325"
			y="34"
			width="230"
			height="252"
			rx="18"
			fill="none"
			stroke="#3b82f6"
			stroke-opacity="0.35"
		/>
		<text x="440" y="150" text-anchor="middle" font-size="18" font-weight="700" fill="currentColor">
			Virtual device
		</text>
		<text
			x="440"
			y="176"
			text-anchor="middle"
			font-size="14"
			fill="currentColor"
			fill-opacity="0.6"
		>
			real hardware to the OS
		</text>

		<g>
			<rect x="10" y="58" width="230" height="72" rx="16" class="diagram-box" stroke-width="1" />
			<text
				x="125"
				y="90"
				text-anchor="middle"
				font-size="17"
				font-weight="600"
				fill="currentColor"
			>
				Zoom, Chrome, a DAW
			</text>
			<text
				x="125"
				y="113"
				text-anchor="middle"
				font-size="13"
				fill="currentColor"
				fill-opacity="0.6"
			>
				plays into it
			</text>
			<path class="wire" d="M248 94 H317" stroke="#10b981" marker-end="url(#vd-arrow-in)" />

			<path class="wire" d="M563 94 H632" stroke="#10b981" marker-end="url(#vd-arrow-in)" />
			<rect x="640" y="58" width="230" height="72" rx="16" class="diagram-box" stroke-width="1" />
			<text
				x="755"
				y="90"
				text-anchor="middle"
				font-size="17"
				font-weight="600"
				fill="currentColor"
			>
				Your graph
			</text>
			<text
				x="755"
				y="113"
				text-anchor="middle"
				font-size="13"
				fill="currentColor"
				fill-opacity="0.6"
			>
				arrives as a source node
			</text>
		</g>

		<g>
			<rect x="10" y="214" width="230" height="72" rx="16" class="diagram-box" stroke-width="1" />
			<text
				x="125"
				y="246"
				text-anchor="middle"
				font-size="17"
				font-weight="600"
				fill="currentColor"
			>
				Your graph
			</text>
			<text
				x="125"
				y="269"
				text-anchor="middle"
				font-size="13"
				fill="currentColor"
				fill-opacity="0.6"
			>
				processed and mixed
			</text>
			<path class="wire" d="M248 250 H317" stroke="#0ea5e9" marker-end="url(#vd-arrow-out)" />

			<path class="wire" d="M563 250 H632" stroke="#0ea5e9" marker-end="url(#vd-arrow-out)" />
			<rect x="640" y="214" width="230" height="72" rx="16" class="diagram-box" stroke-width="1" />
			<text
				x="755"
				y="246"
				text-anchor="middle"
				font-size="17"
				font-weight="600"
				fill="currentColor"
			>
				Any app
			</text>
			<text
				x="755"
				y="269"
				text-anchor="middle"
				font-size="13"
				fill="currentColor"
				fill-opacity="0.6"
			>
				picks it as its microphone
			</text>
		</g>
	</svg>

	<dl class="mt-12 grid gap-6 sm:grid-cols-3">
		{#each specs as spec (spec.label)}
			<div class="text-center">
				<dt class="text-xl font-semibold tracking-tight">{spec.value}</dt>
				<dd class="mt-1 text-sm text-neutral-1000">{spec.label}</dd>
			</div>
		{/each}
	</dl>

	<p class="mt-10 text-center text-xs text-neutral-900">
		Virtual devices are available on macOS and Linux.
	</p>
</section>
