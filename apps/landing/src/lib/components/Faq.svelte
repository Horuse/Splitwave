<script lang="ts">
	import { jsonLdTag } from '#lib/json_ld.js';
	// Drop a screenshot named <file>.webp (or .png) into src/lib/assets/faq/ and it
	// replaces the placeholder; until then the dashed box says what to capture.
	const images = import.meta.glob<string>('../assets/faq/*.{webp,png}', {
		eager: true,
		import: 'default'
	});
	const imageFor = (file: string) =>
		images[`../assets/faq/${file}.webp`] ?? images[`../assets/faq/${file}.png`];

	type Faq = { q: string; a: string; shot?: { file: string; alt: string } };

	const faqs: Faq[] = [
		{
			q: 'What is Splitwave?',
			a: 'A free, open-source desktop audio router for macOS, Linux and Windows. You build a node graph — sources, effects, outputs — and it runs in real time while you rewire it.'
		},
		{
			q: 'How do I set up a virtual audio cable?',
			a: 'Open Virtual devices in Splitwave and click Install. On macOS you enter your password once; on Linux no password is needed. Then add as many named devices as you like — every app sees each one as a microphone and a speaker. On Windows the same button downloads and installs VB-CABLE.',
			shot: {
				file: 'virtual-device',
				alt: 'Virtual devices screen in Splitwave with two named virtual devices'
			}
		},
		{
			q: 'How do I use Splitwave as a microphone in Discord, Zoom or OBS?',
			a: 'Wire Microphone → Mute → a Speaker node set to your virtual device, then pick that virtual device as the microphone in Discord, Zoom, Teams or OBS. Bind a key to the Mute node to mute yourself in all those apps at once.',
			shot: {
				file: 'discord-mic',
				alt: 'Microphone node wired through a Mute node into a Speaker node set to a Splitwave virtual device'
			}
		},
		{
			q: 'How do I route audio through AU, CLAP or VST3 plugins?',
			a: "Add a Plugin node, choose an installed VST3, AU or CLAP plugin, and wire it into the chain like any built-in effect. You can open the plugin's own editor from the node — no DAW or separate plugin host needed.",
			shot: {
				file: 'plugins',
				alt: 'Microphone wired through a CLAP plugin node with its parameters into a virtual device'
			}
		},
		{
			q: 'How do I route audio between apps?',
			a: 'Capture one application with an App Audio node, add effects if you want, and send it to a virtual device. The other app — OBS, your DAW, a call — selects that device as its input.',
			shot: {
				file: 'app-to-app',
				alt: 'App Audio node capturing Google Chrome, wired into a Speaker node set to a virtual device'
			}
		},
		{
			q: 'How do I record system audio?',
			a: 'Wire a System Audio node into a File Recording node. It records WAV, FLAC, AIFF, Opus or MP3 (plus AAC on macOS), and you can mix your microphone into the same file.',
			shot: {
				file: 'record-system-audio',
				alt: 'System Audio node wired into a File Recording node recording WAV with a live waveform'
			}
		},
		{
			q: 'Can one source feed several outputs at once?',
			a: 'Yes. Connect one output to several nodes — headphones, a virtual device and a recording at the same time, each with its own effects.',
			shot: {
				file: 'multi-output',
				alt: 'One App Audio node wired to laptop speakers, a virtual device and AirPods at the same time'
			}
		},
		{
			q: 'Can I stream audio to another computer?',
			a: 'Yes. A UDP sender and receiver pair, or a WebRTC node, carries audio between machines. Audio leaves your computer only through network nodes you add yourself.'
		},
		{
			q: 'Does Splitwave work with my DAW?',
			a: 'Yes. Send audio to a virtual device and pick it as the input in Logic Pro, Ableton Live, Reaper or any other DAW.'
		},
		{
			q: 'Which platforms are supported?',
			a: 'macOS 13 Ventura or newer (Apple Silicon and Intel), Linux x86_64 with PipeWire, and Windows 10 and 11 (64-bit).'
		},
		{
			q: 'Does it need a kernel extension or my password on macOS?',
			a: 'No kernel extension. macOS asks for your password once, when Splitwave installs its userspace audio driver for virtual devices.'
		},
		{
			q: 'Does Splitwave send my audio anywhere?',
			a: 'No. Everything is processed locally, with no account and no analytics. The only routine request is a check for new releases on GitHub.'
		},
		{
			q: 'Is Splitwave an alternative to BlackHole, Loopback, Audio Hijack or VoiceMeeter?',
			a: 'It covers many of the same jobs — virtual devices, app-to-app routing, mic processing and recording — in one node graph. Each tool has its own features, so check that Splitwave fits your workflow. Splitwave is independent and not affiliated with the makers of these products.'
		},
		{
			q: 'Is Splitwave free?',
			a: 'Yes. It is free and open source under the MIT license, with the source code on GitHub.'
		}
	];

	const jsonLd = jsonLdTag({
		'@context': 'https://schema.org',
		'@type': 'FAQPage',
		mainEntity: faqs.map(({ q, a }) => ({
			'@type': 'Question',
			name: q,
			acceptedAnswer: { '@type': 'Answer', text: a }
		}))
	});
</script>

<svelte:head>
	{@html jsonLd}
</svelte:head>

<section class="mx-auto max-w-3xl px-6 pb-24">
	<h2 class="mb-8 text-center text-3xl font-bold tracking-tight">Frequently asked questions</h2>
	<div class="flex flex-col">
		{#each faqs as { q, a, shot } (q)}
			<details class="group border-b border-neutral-200 py-4">
				<summary
					class="flex cursor-pointer list-none items-center justify-between gap-4 text-lg font-semibold text-theme [&::-webkit-details-marker]:hidden"
				>
					<h3>{q}</h3>
					<span aria-hidden="true" class="shrink-0 transition-transform group-open:rotate-45">
						+
					</span>
				</summary>
				<p class="mt-2 leading-relaxed text-neutral-1000">{a}</p>
				{#if shot}
					{@const src = imageFor(shot.file)}
					{#if src}
						<img
							{src}
							alt={shot.alt}
							loading="lazy"
							class="mt-4 w-full rounded-xl border border-neutral-200"
						/>
					{:else if import.meta.env.DEV}
						<div
							class="mt-4 flex aspect-video w-full flex-col items-center justify-center gap-1 rounded-xl border-2 border-dashed border-neutral-300 p-4 text-center text-sm text-neutral-900"
						>
							<span class="font-mono">src/lib/assets/faq/{shot.file}.webp</span>
							<span>{shot.alt}</span>
						</div>
					{/if}
				{/if}
			</details>
		{/each}
	</div>
</section>
