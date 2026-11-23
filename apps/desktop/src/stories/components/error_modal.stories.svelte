<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import ErrorModal from '$lib/modules/error/ui/error_modal.svelte';
	import { errorStore, type ErrorEntry } from '$lib/modules/error/stores.svelte';
	import StoreScope from '../harness/store_scope.svelte';

	const { Story } = defineMeta({ title: 'Components/Error Modal', component: ErrorModal });

	const CRASH: ErrorEntry = {
		source: 'nativeCrash',
		message: 'Native crash: SIGSEGV (Process crashed during audio startup)',
		stack: '0 splitwave 0x0001 audio::engine::start\n1 splitwave 0x0002 main',
		logs: '[info] engine starting\n[warn] device busy',
		at: 0,
		previousRun: true
	};
	const LIVE: ErrorEntry = { ...CRASH, source: 'jsError', message: "TypeError: Cannot read properties of undefined (reading 'id')", previousRun: false };
</script>

<Story name="Crashed last time" asChild>
	<StoreScope apply={() => (errorStore.current = CRASH)}><ErrorModal /></StoreScope>
</Story>
<Story name="Live error" asChild>
	<StoreScope apply={() => (errorStore.current = LIVE)}><ErrorModal /></StoreScope>
</Story>
