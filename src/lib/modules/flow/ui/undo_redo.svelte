<script lang="ts">
	import { Button } from '@splitwave/ui';
	import { onMount } from 'svelte';
	import { pipelineStore } from '$lib/modules/pipeline/stores.svelte';
	import ArrowUndo from '~icons/fluent/arrow-undo-16-filled';
	import ArrowRedo from '~icons/fluent/arrow-redo-16-filled';

	let canUndo = $derived(pipelineStore.editorActions?.canUndo?.() ?? false);
	let canRedo = $derived(pipelineStore.editorActions?.canRedo?.() ?? false);

	let undoActive = $state(false);
	let redoActive = $state(false);

	function flash(setter: (v: boolean) => void) {
		setter(true);
		setTimeout(() => setter(false), 100);
	}

	function onUndo() {
		pipelineStore.editorActions?.undo();
	}
	function onRedo() {
		pipelineStore.editorActions?.redo();
	}

	function onWindowKeyDown(e: KeyboardEvent) {
		if (!(e.metaKey || e.ctrlKey)) return;
		if (e.key !== 'z' && e.key !== 'Z') return;
		const t = e.target as HTMLElement | null;
		const tag = t?.tagName?.toLowerCase();
		if (tag === 'input' || tag === 'textarea' || tag === 'select' || t?.isContentEditable) return;
		e.preventDefault();
		if (e.shiftKey) {
			if (!canRedo) return;
			flash((v) => (redoActive = v));
			onRedo();
		} else {
			if (!canUndo) return;
			flash((v) => (undoActive = v));
			onUndo();
		}
	}

	onMount(() => {
		window.addEventListener('keydown', onWindowKeyDown, { capture: true });
		return () => window.removeEventListener('keydown', onWindowKeyDown, { capture: true });
	});
</script>

<div class="flex items-center gap-1">
	<Button icon active={undoActive} type="button" title="Undo (⌘Z)" disabled={!canUndo} onclick={onUndo}>
		<ArrowUndo class="h-4 w-4" />
	</Button>
	<Button icon active={redoActive} type="button" title="Redo (⇧⌘Z)" disabled={!canRedo} onclick={onRedo}>
		<ArrowRedo class="h-4 w-4" />
	</Button>
</div>
