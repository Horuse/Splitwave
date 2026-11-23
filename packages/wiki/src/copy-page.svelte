<script lang="ts">
	import { Menu, MenuItem, MenuSeparator, Popover } from '@splitwave/ui';
	import IconCopy from '~icons/fluent/copy-16-regular';
	import IconCheck from '~icons/fluent/checkmark-16-regular';
	import IconChevron from '~icons/fluent/chevron-down-16-regular';
	import IconLink from '~icons/fluent/link-16-regular';
	import IconChat from '~icons/fluent/chat-sparkle-16-regular';

	interface Props {
		markdown: string;
		/** Public URL of the page, which AI tools are asked to read. */
		url: string;
		/** How a link opens: a new tab on the web, the system browser in the app. */
		open?: (href: string) => void;
	}

	let { markdown, url, open: openLink = (href) => window.open(href, '_blank', 'noopener,noreferrer') }: Props = $props();

	let menuOpen = $state(false);
	let copied = $state(false);
	let timer: ReturnType<typeof setTimeout> | undefined;

	async function copy(text: string) {
		await navigator.clipboard.writeText(text);
		copied = true;
		clearTimeout(timer);
		timer = setTimeout(() => (copied = false), 1500);
	}

	const prompt = $derived(`Read ${url} so I can ask questions about it.`);

	const AI = $derived([
		{ label: 'Open in ChatGPT', href: `https://chatgpt.com/?hints=search&q=${encodeURIComponent(prompt)}` },
		{ label: 'Open in Claude', href: `https://claude.ai/new?q=${encodeURIComponent(prompt)}` },
		{ label: 'Open in Perplexity', href: `https://www.perplexity.ai/search?q=${encodeURIComponent(prompt)}` }
	]);

	function pick(action: () => unknown) {
		menuOpen = false;
		action();
	}

	const SEGMENT = 'flex h-8 items-center gap-1.5 text-xs text-neutral-1100 hover:bg-neutral-200';
</script>

<div class="not-prose inline-flex overflow-hidden rounded-lg border border-neutral-300 bg-neutral-100">
	<button type="button" class={[SEGMENT, 'px-2.5']} onclick={() => copy(markdown)}>
		{#if copied}<IconCheck class="size-4" />{:else}<IconCopy class="size-4" />{/if}
		{copied ? 'Copied' : 'Copy page'}
	</button>
	<Popover bind:open={menuOpen} placement="bottom-end">
		{#snippet trigger()}
			<button type="button" class={[SEGMENT, 'border-l border-neutral-300 px-1.5']} aria-label="More copy options" aria-expanded={menuOpen}>
				<IconChevron class="size-4" />
			</button>
		{/snippet}
		<Menu>
			<MenuItem label="Copy as Markdown" icon={IconCopy} onclick={() => pick(() => copy(markdown))} />
			<MenuItem label="Copy link" icon={IconLink} onclick={() => pick(() => copy(url))} />
			<MenuSeparator />
			{#each AI as item (item.label)}
				<MenuItem label={item.label} icon={IconChat} onclick={() => pick(() => openLink(item.href))} />
			{/each}
		</Menu>
	</Popover>
</div>
