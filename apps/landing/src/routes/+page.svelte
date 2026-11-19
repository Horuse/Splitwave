<script lang="ts">
	import Hero from '#lib/components/Hero.svelte';
	import NodeCloud from '#lib/components/NodeCloud.svelte';
	import VirtualDevices from '#lib/components/VirtualDevices.svelte';
	import Features from '#lib/components/Features.svelte';
	import Plugins from '#lib/components/Plugins.svelte';
	import Network from '#lib/components/Network.svelte';
	import Platforms from '#lib/components/Platforms.svelte';
	import Faq from '#lib/components/Faq.svelte';
	import FinalCta from '#lib/components/FinalCta.svelte';

	let { data } = $props();

	const jsonLd = $derived(
		JSON.stringify({
			'@context': 'https://schema.org',
			'@type': 'SoftwareApplication',
			name: 'Splitwave',
			operatingSystem: 'macOS 13+, Linux, Windows 10+',
			applicationCategory: 'MultimediaApplication',
			offers: { '@type': 'Offer', price: '0', priceCurrency: 'USD' },
			isAccessibleForFree: true,
			description:
				'Free, open-source, node-based audio routing for macOS, Windows and Linux. Route microphones, system audio and apps through effects, plugins and virtual audio devices.',
			keywords:
				'audio routing, audio router, macOS audio routing app, virtual audio cable, virtual audio cable setup, virtual audio device, virtual microphone, route audio between apps, app audio capture, record system audio, system audio capture, VST3 plugin routing, VST3 host, VST plugins for microphone, CLAP plugin host, AU plugin host, split audio to multiple outputs, stream audio over network, noise suppression, BlackHole alternative, Loopback alternative, Audio Hijack alternative, VoiceMeeter alternative',
			featureList: [
				'Node-based audio routing graph',
				'Unlimited virtual audio devices',
				'System audio and per-app audio capture',
				'Microphone capture with noise suppression, gate, compressor and EQ',
				'CLAP, AU and VST3 plugin hosting',
				'Multi-format recording (WAV, FLAC, AIFF, Opus, MP3; AAC on macOS)',
				'Audio streaming over the network via WebRTC and UDP'
			],
			url: 'https://splitwave.app',
			downloadUrl: 'https://splitwave.app/download',
			softwareVersion: data.release?.version,
			screenshot: 'https://splitwave.app/preview.webp',
			license: 'https://opensource.org/licenses/MIT',
			sameAs: ['https://github.com/Horuse/Splitwave', 'https://www.patreon.com/splitwave']
		})
	);
</script>

<svelte:head>
	{@html `<script type="application/ld+json">${jsonLd}</script>`}
</svelte:head>

<main class="flex flex-col gap-16">
	<Hero release={data.release} />
	<NodeCloud />
	<VirtualDevices />
	<Features />
	<Plugins />
	<Network />
	<Platforms />
	<Faq />
	<FinalCta />
</main>
