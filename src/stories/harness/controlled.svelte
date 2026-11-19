<script lang="ts">
	import type { Component } from 'svelte';
	import { setEnginePort } from '@splitwave/nodes/engine';
	import { appEnginePort } from '$lib/modules/audio/engine_port.svelte';

	interface Props {
		component: Component<any>;
		props: Record<string, unknown>;
		/** Prop that carries the value, and the callback that reports a new one. */
		valueKey?: string;
		changeKey?: string;
		/** Storybook action spy, so changes show up in the Actions panel. */
		onChange?: (v: unknown) => void;
	}
	let { component: Target, props, valueKey = 'value', changeKey = 'onChange', onChange }: Props = $props();

	setEnginePort(appEnginePort);

	// Controlled inputs only move when their owner writes back; the story is that owner.
	let value = $state<unknown>();
	$effect.pre(() => {
		value = props[valueKey];
	});

	function change(next: unknown) {
		value = next;
		onChange?.(next);
	}
</script>

<Target {...props} {...{ [valueKey]: value, [changeKey]: change }} />
