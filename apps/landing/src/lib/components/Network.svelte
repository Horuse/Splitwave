<script lang="ts">
	import WireDown from '#lib/components/WireDown.svelte';

	// Wording tracks the node registry descriptions in flow/utils/nodes.ts.
	const modes = [
		{ label: 'WebRTC', text: 'Peers connect both ways. Each one arrives as its own source node.' },
		{ label: 'Net Sender', text: 'Push a signal over UDP to a direct IP, as Opus or raw PCM.' },
		{ label: 'Net Receiver', text: 'Pull a signal over UDP from a direct IP, as Opus or raw PCM.' }
	];
</script>

<section class="mx-auto max-w-5xl px-6 pb-24">
	<div class="mb-12 text-center">
		<h2 class="text-3xl font-bold tracking-tight">The graph can leave the machine</h2>
		<p class="mx-auto mt-3 max-w-xl text-pretty text-neutral-1000">
			A cable can run to another computer. Whatever comes back shows up as an ordinary source node,
			no different from a microphone.
		</p>
	</div>

	<!-- SVG text does not reflow, so narrow screens get the same diagram as a
	     stacked column of real HTML instead of unreadable shrunken labels. -->
	<div class="flex flex-col items-center md:hidden">
		<div class="w-full rounded-2xl border border-dashed border-neutral-300 p-4">
			<p class="mb-3 text-xs font-semibold text-neutral-900">THIS MACHINE</p>
			<div class="rounded-xl border border-neutral-300 bg-neutral-100 px-5 py-4 text-center">
				<p class="font-semibold">Your graph</p>
				<p class="mt-1 text-sm text-neutral-1000">inputs, effects, outputs</p>
			</div>
		</div>

		<WireDown color="#f43f5e" both />

		<div class="rounded-xl border border-rose-500/35 bg-neutral-100 px-6 py-3 text-center">
			<p class="text-sm font-semibold">UDP &middot; WebRTC</p>
		</div>

		<WireDown color="#f43f5e" both />

		<div class="w-full rounded-2xl border border-dashed border-neutral-300 p-4">
			<p class="mb-3 text-xs font-semibold text-neutral-900">ANYWHERE ELSE</p>
			<div class="flex flex-col gap-3">
				<div class="rounded-xl border border-neutral-300 bg-neutral-100 px-5 py-4 text-center">
					<p class="font-semibold">Peer receiving your mix</p>
				</div>
				<div class="rounded-xl border border-neutral-300 bg-neutral-100 px-5 py-4 text-center">
					<p class="font-semibold">Peer sending theirs back</p>
				</div>
			</div>
		</div>
	</div>

	<svg
		viewBox="0 0 880 300"
		class="hidden w-full text-theme md:block"
		role="img"
		aria-label="Your graph sends audio over UDP or WebRTC to remote peers, and each peer returns as a source node"
	>
		<defs>
			<marker
				id="net-arrow"
				viewBox="0 0 8 8"
				refX="7"
				refY="4"
				markerWidth="6"
				markerHeight="6"
				orient="auto"
			>
				<path d="M0 0 L8 4 L0 8 z" fill="#f43f5e" />
			</marker>
			<marker
				id="net-arrow-back"
				viewBox="0 0 8 8"
				refX="7"
				refY="4"
				markerWidth="6"
				markerHeight="6"
				orient="auto"
			>
				<path d="M0 0 L8 4 L0 8 z" fill="#f43f5e" fill-opacity="0.6" />
			</marker>
		</defs>

		<rect
			x="10"
			y="30"
			width="330"
			height="240"
			rx="20"
			fill="none"
			class="stroke-neutral-300"
			stroke-dasharray="6 6"
		/>
		<text x="34" y="58" font-size="11" font-weight="600" fill="currentColor" fill-opacity="0.5">
			THIS MACHINE
		</text>
		<rect x="45" y="120" width="265" height="60" rx="14" class="diagram-box" stroke-width="1" />
		<text x="177" y="146" text-anchor="middle" font-size="14" font-weight="600" fill="currentColor">
			Your graph
		</text>
		<text
			x="177"
			y="165"
			text-anchor="middle"
			font-size="11"
			fill="currentColor"
			fill-opacity="0.55"
		>
			inputs, effects, outputs
		</text>

		<path class="wire" d="M318 138 H392" stroke="#f43f5e" marker-end="url(#net-arrow)" />
		<path
			class="wire"
			d="M472 168 H318"
			stroke="#f43f5e"
			stroke-opacity="0.6"
			marker-end="url(#net-arrow-back)"
		/>

		<rect x="400" y="126" width="80" height="52" rx="14" class="diagram-box" stroke-width="1" />
		<rect
			x="400"
			y="126"
			width="80"
			height="52"
			rx="14"
			fill="none"
			stroke="#f43f5e"
			stroke-opacity="0.35"
		/>
		<text x="440" y="147" text-anchor="middle" font-size="11" font-weight="600" fill="currentColor">
			UDP
		</text>
		<text x="440" y="163" text-anchor="middle" font-size="11" font-weight="600" fill="currentColor">
			WebRTC
		</text>

		<path
			class="wire"
			d="M488 138 C 528 138, 534 92, 572 92"
			stroke="#f43f5e"
			marker-end="url(#net-arrow)"
		/>
		<path
			class="wire"
			d="M572 208 C 534 208, 528 168, 488 168"
			stroke="#f43f5e"
			stroke-opacity="0.6"
			marker-end="url(#net-arrow-back)"
		/>

		<rect
			x="540"
			y="30"
			width="330"
			height="240"
			rx="20"
			fill="none"
			class="stroke-neutral-300"
			stroke-dasharray="6 6"
		/>
		<text x="564" y="58" font-size="11" font-weight="600" fill="currentColor" fill-opacity="0.5">
			ANYWHERE ELSE
		</text>
		<rect x="580" y="66" width="255" height="52" rx="14" class="diagram-box" stroke-width="1" />
		<text x="707" y="97" text-anchor="middle" font-size="13" font-weight="600" fill="currentColor">
			Peer receiving your mix
		</text>
		<rect x="580" y="182" width="255" height="52" rx="14" class="diagram-box" stroke-width="1" />
		<text x="707" y="213" text-anchor="middle" font-size="13" font-weight="600" fill="currentColor">
			Peer sending theirs back
		</text>
	</svg>

	<dl class="mt-10 grid gap-x-8 gap-y-4 sm:grid-cols-3">
		{#each modes as mode (mode.label)}
			<div>
				<dt class="flex items-center gap-2 text-sm font-semibold text-neutral-1100">
					<span class="size-2 rounded-full bg-rose-500"></span>
					{mode.label}
				</dt>
				<dd class="mt-2 text-sm text-pretty text-neutral-1000">{mode.text}</dd>
			</div>
		{/each}
	</dl>
</section>
