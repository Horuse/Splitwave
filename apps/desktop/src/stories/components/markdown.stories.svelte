<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import Markdown from '$lib/components/markdown.svelte';

	const ARTICLE = `# Splitwave 2.0 is here

A **new node editor**, *faster* graphs and ~~old~~ cleaner menus. Read the [full notes](https://github.com).

> [!TIP]
> Collapse big nodes with the chevron in their header.

## What changed

- [x] Shared parameter graphs
- [x] Live input level on dynamics
- [ ] Cue Pad node
  - preloaded slots
  - control sockets per slot

1. Open a pipeline
2. Press **Run**

| Node | Latency | Notes |
|:-----|-------:|:-----:|
| Limiter | 5 ms | lookahead |
| Gain | 0 ms | \`instant\` |

\`\`\`ts
const port = createDemoPort();
setEnginePort(port);
\`\`\`

\`\`\`mermaid
flowchart LR
  Mic[Microphone] --> Gate[Noise Gate] --> Comp[Compressor] --> Out[Speaker]
\`\`\`

> Quotes keep their own blocks
> - even lists

---

Line one with a hard break\\
line two.`;

	const { Story } = defineMeta({
		title: 'Components/Markdown',
		component: Markdown,
		argTypes: { variant: { control: 'inline-radio', options: ['compact', 'article'] }, source: { control: 'text' } },
		args: { source: ARTICLE, variant: 'article' }
	});
</script>

{#snippet template(args: Record<string, any>)}
	<div class="max-w-2xl p-8"><Markdown source={args.source} variant={args.variant} class="text-sm text-neutral-1100" /></div>
{/snippet}

<Story name="Article" {template} />
<Story name="Compact" args={{ variant: 'compact' }} {template} />
