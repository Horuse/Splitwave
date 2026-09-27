<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import NodeCanvas from '../harness/node_canvas.svelte';
	import { compose, dataArgs, envArgs, wiringArgs } from '../harness/args';
	import { DEFAULT_ENV, mockPlugins } from '../harness/backend';

	// The node matches a scan result by path + pluginId, so a story picks one of
	// the mocked plugins by index and gets the whole descriptor.
	const CATALOG = mockPlugins({ ...DEFAULT_ENV, plugins: 6 });

	function withPlugin({ plugin, ...rest }: Record<string, unknown>): Record<string, unknown> {
		const desc = CATALOG[Number(plugin) - 1];
		if (!desc) return { ...rest, format: null, path: '', pluginId: '' };
		return { ...rest, format: desc.format, path: desc.path, pluginId: desc.pluginId, name: desc.name, vendor: desc.vendor };
	}

	const { argTypes, args } = compose(
		dataArgs(
			{
				plugin: {
					control: 'select',
					options: [0, ...CATALOG.map((_, i) => i + 1)],
					labels: Object.fromEntries([[0, 'None'], ...CATALOG.map((p, i) => [i + 1, p.name])])
				},
				showParams: { control: 'boolean' },
				bypassed: { control: 'boolean' }
			},
			{ plugin: 1, showParams: false, bypassed: false }
		),
		wiringArgs(2),
		envArgs(['running', 'plugins', 'pluginParams', 'pluginEditor'])
	);

	const { Story } = defineMeta({ title: 'Nodes/Effects/Plugin', argTypes, args });
</script>

{#snippet template(a: Record<string, unknown>)}
	<NodeCanvas kind="plugin" args={withPlugin(a)} />
{/snippet}

<Story name="Not chosen" args={{ plugin: 0 }} {template} />
<Story name="Chosen, stopped" {template} />
<Story name="Running" args={{ running: true }} {template} />
<Story name="Running with params" args={{ running: true, showParams: true }} {template} />
<Story name="Many params" args={{ running: true, showParams: true, pluginParams: 10 }} {template} />
<Story name="No editor" args={{ running: true, pluginEditor: false }} {template} />
<Story name="AU plugin" args={{ plugin: 3, running: true }} {template} />
<Story name="No plugins installed" args={{ plugin: 0, plugins: 0 }} {template} />
<Story name="Bypassed" args={{ bypassed: true }} {template} />
