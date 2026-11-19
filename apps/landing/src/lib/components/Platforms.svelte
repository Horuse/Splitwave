<script lang="ts">
	import * as Icons from '#lib/components/icons/index.js';

	// Mirrors the platform support table in the Splitwave README.
	const platforms = [
		{ name: 'macOS', icon: Icons.Apple },
		{ name: 'Linux', icon: Icons.Linux },
		{ name: 'Windows', icon: Icons.Windows }
	];

	const rows: { feature: string; values: [string, string, string] }[] = [
		{
			feature: 'Runs on',
			values: [
				'macOS 13 Ventura or newer, Apple Silicon and Intel',
				'x86_64 with PipeWire',
				'Windows 10 2004 or newer and 11, 64-bit'
			]
		},
		{
			feature: 'Virtual devices',
			values: [
				'As many as you need, with your own names',
				'As many as you need, with your own names',
				'One VB-CABLE virtual microphone, installed from the app'
			]
		},
		{
			feature: 'Password to install',
			values: ['Once, for the audio driver', 'Not needed', 'Administrator approval for VB-CABLE']
		},
		{ feature: 'System audio capture', values: ['Yes', 'Yes', 'Yes'] },
		{ feature: 'Per-app audio capture', values: ['Yes', 'Yes', 'Yes'] },
		{ feature: 'Plugins', values: ['AU, CLAP, VST3', 'CLAP, VST3', 'CLAP, VST3'] },
		{
			feature: 'Recording formats',
			values: [
				'WAV, FLAC, AIFF, MP3, Opus, AAC',
				'WAV, FLAC, AIFF, MP3, Opus',
				'WAV, FLAC, AIFF, MP3, Opus'
			]
		}
	];
</script>

<section class="mx-auto max-w-5xl px-6 pb-24">
	<div class="mb-12 text-center">
		<h2 class="text-3xl font-bold tracking-tight">One app on macOS, Linux and Windows</h2>
		<p class="mx-auto mt-3 max-w-xl text-pretty text-neutral-1000">
			The same graph, nodes and effects everywhere. What differs is how each system lets audio in
			and out.
		</p>
	</div>

	<!-- A table reads well from tablet width up; phones get one card per platform. -->
	<table class="hidden w-full border-collapse text-left text-sm md:table">
		<thead>
			<tr class="border-b border-neutral-300">
				<th scope="col" class="w-1/4 py-3 pr-4 font-normal text-neutral-900"
					><span class="sr-only">Feature</span></th
				>
				{#each platforms as p (p.name)}
					<th scope="col" class="w-1/4 px-4 py-3 font-semibold">
						<span class="inline-flex items-center gap-2"><p.icon class="size-4" />{p.name}</span>
					</th>
				{/each}
			</tr>
		</thead>
		<tbody>
			{#each rows as row (row.feature)}
				<tr class="border-b border-dashed border-neutral-300 last:border-0">
					<th scope="row" class="py-3 pr-4 font-medium text-neutral-900">{row.feature}</th>
					{#each row.values as value, i (i)}
						<td class="px-4 py-3 text-neutral-1000">{value}</td>
					{/each}
				</tr>
			{/each}
		</tbody>
	</table>

	<div class="flex flex-col gap-4 md:hidden">
		{#each platforms as p, i (p.name)}
			<div class="rounded-2xl border border-dashed border-neutral-300 p-4">
				<p class="mb-3 inline-flex items-center gap-2 font-semibold">
					<p.icon class="size-4" />{p.name}
				</p>
				<dl class="flex flex-col gap-2 text-sm">
					{#each rows as row (row.feature)}
						<div class="flex justify-between gap-4">
							<dt class="text-neutral-900">{row.feature}</dt>
							<dd class="text-right text-neutral-1000">{row.values[i]}</dd>
						</div>
					{/each}
				</dl>
			</div>
		{/each}
	</div>
</section>
