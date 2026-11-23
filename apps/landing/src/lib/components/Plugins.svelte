<script lang="ts">
	import WireDown from '#lib/components/WireDown.svelte';

	// Same accent as the Plugins stage in Features, so the colour keeps its meaning.
	const formats = [
		{ label: 'CLAP', text: 'The open format, with per-parameter modulation built in.' },
		{ label: 'AU', text: 'Audio Units from your existing macOS library.' },
		{ label: 'VST3', text: 'The format most third-party effects still ship in.' }
	];

	const specs = [
		{ value: 'Native editor', label: 'opens straight from the node' },
		{ value: 'Automatable', label: 'parameters exposed on the node' },
		{ value: 'Full node width', label: 'if the plugin takes it, stereo pairs if not' }
	];
</script>

<section class="mx-auto max-w-5xl px-6 pb-24">
	<div class="mb-12 text-center">
		<h2 class="text-3xl font-bold tracking-tight">VST3, AU and CLAP plugins are just more nodes</h2>
		<p class="mx-auto mt-3 max-w-xl text-pretty text-neutral-1000">
			Point a Plugin node at anything already installed on the machine &mdash; CLAP, AU or VST3
			&mdash; and wire it into the chain like a built-in effect. Same cables, same signal.
		</p>
	</div>

	<!-- SVG text does not reflow, so narrow screens get the same diagram as a
	     stacked column of real HTML instead of unreadable shrunken labels. -->
	<div class="flex flex-col items-center md:hidden">
		<div class="flex w-full flex-wrap justify-center gap-2">
			{#each formats as f (f.label)}
				<div
					class="rounded-xl border border-neutral-300 bg-neutral-100 px-4 py-2 text-sm font-semibold"
				>
					{f.label}
				</div>
			{/each}
		</div>

		<WireDown color="#d946ef" />

		<div
			class="w-full rounded-2xl border border-fuchsia-500/35 bg-neutral-100 px-5 py-4 text-center"
		>
			<p class="font-semibold">Plugin node</p>
			<p class="mt-1 text-sm text-neutral-1000">hosts it in the graph</p>
		</div>

		<WireDown color="#d946ef" />

		<div class="w-full rounded-2xl border border-neutral-300 bg-neutral-100 px-5 py-4 text-center">
			<p class="font-semibold">The rest of your chain</p>
			<p class="mt-1 text-sm text-neutral-1000">meters, outputs, peers</p>
		</div>
	</div>

	<svg
		viewBox="0 0 880 300"
		class="hidden w-full text-theme md:mx-auto md:block md:w-2xl lg:w-4xl"
		role="img"
		aria-label="CLAP, AU and VST3 plugins installed on the machine are hosted by a Plugin node, which feeds the rest of your chain"
	>
		<defs>
			<marker
				id="pl-arrow"
				viewBox="0 0 8 8"
				refX="7"
				refY="4"
				markerWidth="6"
				markerHeight="6"
				orient="auto"
			>
				<path d="M0 0 L8 4 L0 8 z" fill="#d946ef" />
			</marker>
		</defs>

		<rect
			x="10"
			y="30"
			width="280"
			height="240"
			rx="20"
			fill="none"
			class="stroke-neutral-300"
			stroke-dasharray="6 6"
		/>
		<text x="34" y="58" font-size="11" font-weight="600" fill="currentColor" fill-opacity="0.5">
			INSTALLED ON THIS MACHINE
		</text>

		<rect x="45" y="76" width="210" height="52" rx="14" class="diagram-box" stroke-width="1" />
		<text x="150" y="107" text-anchor="middle" font-size="15" font-weight="600" fill="currentColor">
			CLAP
		</text>
		<rect x="45" y="138" width="210" height="52" rx="14" class="diagram-box" stroke-width="1" />
		<text x="150" y="169" text-anchor="middle" font-size="15" font-weight="600" fill="currentColor">
			AU
		</text>
		<rect x="45" y="200" width="210" height="52" rx="14" class="diagram-box" stroke-width="1" />
		<text x="150" y="231" text-anchor="middle" font-size="15" font-weight="600" fill="currentColor">
			VST3
		</text>

		<path
			class="wire"
			d="M263 102 C 320 102, 320 164, 372 164"
			stroke="#d946ef"
			marker-end="url(#pl-arrow)"
		/>
		<path class="wire" d="M263 164 H372" stroke="#d946ef" marker-end="url(#pl-arrow)" />
		<path
			class="wire"
			d="M263 226 C 320 226, 320 164, 372 164"
			stroke="#d946ef"
			marker-end="url(#pl-arrow)"
		/>

		<rect x="380" y="118" width="200" height="92" rx="18" class="diagram-box" stroke-width="1" />
		<rect
			x="380"
			y="118"
			width="200"
			height="92"
			rx="18"
			fill="none"
			stroke="#d946ef"
			stroke-opacity="0.35"
		/>
		<text x="480" y="156" text-anchor="middle" font-size="17" font-weight="700" fill="currentColor">
			Plugin node
		</text>
		<text
			x="480"
			y="180"
			text-anchor="middle"
			font-size="13"
			fill="currentColor"
			fill-opacity="0.6"
		>
			hosts it in the graph
		</text>

		<path class="wire" d="M588 164 H647" stroke="#d946ef" marker-end="url(#pl-arrow)" />

		<rect x="655" y="138" width="215" height="52" rx="14" class="diagram-box" stroke-width="1" />
		<text x="762" y="162" text-anchor="middle" font-size="15" font-weight="600" fill="currentColor">
			The rest of your chain
		</text>
		<text
			x="762"
			y="180"
			text-anchor="middle"
			font-size="11"
			fill="currentColor"
			fill-opacity="0.55"
		>
			meters, outputs, peers
		</text>
	</svg>

	<dl class="mt-10 grid gap-x-8 gap-y-4 sm:grid-cols-3">
		{#each formats as format (format.label)}
			<div>
				<dt class="flex items-center gap-2 text-sm font-semibold text-neutral-1100">
					<span class="size-2 rounded-full bg-fuchsia-500"></span>
					{format.label}
				</dt>
				<dd class="mt-2 text-sm text-pretty text-neutral-1000">{format.text}</dd>
			</div>
		{/each}
	</dl>

	<dl class="mt-12 grid gap-6 sm:grid-cols-3">
		{#each specs as spec (spec.label)}
			<div class="text-center">
				<dt class="text-xl font-semibold tracking-tight">{spec.value}</dt>
				<dd class="mt-1 text-sm text-neutral-1000">{spec.label}</dd>
			</div>
		{/each}
	</dl>

	<p class="mx-auto mt-10 max-w-2xl text-center text-xs text-pretty text-neutral-900">
		A plugin is first offered the node's full channel count, and one instance handles everything. If
		it refuses that layout, the signal runs as stereo pairs with an instance per pair, and every
		instance follows the same parameter changes. Audio Units are a macOS format, so AU hosting is
		macOS-only.
	</p>
</section>
