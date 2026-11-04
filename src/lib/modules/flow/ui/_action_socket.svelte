<script lang="ts">
	import { getContext, untrack } from 'svelte';
	import { useConnection, useUpdateNodeInternals } from '@xyflow/svelte';
	import ControlSocket from './_control_socket.svelte';
	import { PREVIEW_CTX, controlHandle, signalOf } from '$lib/modules/flow/utils';
	import { useControlInput } from '../control.svelte';

	interface Props {
		nodeId: string;
		/** The button's control input is named after it: `control:<action>`. */
		action: string;
		/** Runs on a rising edge, as a click would. */
		onTrigger: () => void;
	}
	let { nodeId, action, onTrigger }: Props = $props();

	const handle = untrack(() => controlHandle(action));
	const isPreview = getContext(PREVIEW_CTX) === true;
	const input = isPreview
		? null
		: useControlInput(
				untrack(() => nodeId),
				handle
			);
	const connection = useConnection();
	const updateNodeInternals = useUpdateNodeInternals();

	let high = $derived(input?.current ?? false);

	// Hidden until a control cable is dragged or one is already wired, so a
	// node with many buttons does not wear a socket on each of them.
	let shown = $derived.by(() => {
		if (input?.current != null) return true;
		const c = connection.current;
		return c.inProgress && c.fromHandle?.type === 'source' && c.fromNode?.id !== nodeId && signalOf(c.fromHandle?.id) === 'control';
	});

	let wasHigh = false;
	$effect(() => {
		if (high && !wasHigh) untrack(onTrigger);
		wasHigh = high;
	});

	// The socket rides the node's top border above its button; the button
	// moves whenever the node's layout does, so follow the node's size.
	let anchor: HTMLElement | undefined = $state();
	let lift = $state(0);
	$effect(() => {
		const node = anchor?.closest<HTMLElement>('.svelte-flow__node');
		if (!anchor || !node || isPreview) return;
		const measure = () => {
			const zoom = node.getBoundingClientRect().height / node.offsetHeight || 1;
			const next = Math.round((anchor!.getBoundingClientRect().top - node.getBoundingClientRect().top) / zoom);
			if (next === lift) return;
			lift = next;
			requestAnimationFrame(() => updateNodeInternals(nodeId));
		};
		const observer = new ResizeObserver(measure);
		observer.observe(node);
		measure();
		return () => observer.disconnect();
	});
</script>

<span bind:this={anchor} class={['pointer-events-none absolute inset-0', !shown && 'invisible']}>
	<ControlSocket id={handle} side="target" placement="top" {lift} {high} />
</span>
