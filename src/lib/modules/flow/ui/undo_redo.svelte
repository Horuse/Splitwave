<script lang="ts">
	import { onMount } from 'svelte';
	import { pipelineStore } from '$lib/modules/pipeline/stores.svelte';
	import { ArrowUndo, ArrowRedo } from '$lib/components/icons';
	import { t } from '$lib/modules/i18n';

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
		const el = e.target as HTMLElement | null;
		const tag = el?.tagName?.toLowerCase();
		if (tag === 'input' || tag === 'textarea' || tag === 'select' || el?.isContentEditable) return;
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
	<button type="button" class="button-header size-7" class:active={undoActive} title={t('flow.undoShortcut')} disabled={!canUndo} onclick={onUndo}>
		<ArrowUndo class="h-4 w-4" />
	</button>
	<button type="button" class="button-header size-7" class:active={redoActive} title={t('flow.redoShortcut')} disabled={!canRedo} onclick={onRedo}>
		<ArrowRedo class="h-4 w-4" />
	</button>
</div>
