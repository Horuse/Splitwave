<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import UpdateBanner from '$lib/modules/updater/ui/update_banner.svelte';
	import { updaterStore, type UpdateState } from '$lib/modules/updater/stores.svelte';
	import StoreScope from '../harness/store_scope.svelte';

	const { Story } = defineMeta({ title: 'Components/Update Modal', component: UpdateBanner });

	const update = { version: '1.5.0' } as Extract<UpdateState, { phase: 'available' }>['update'];
	const NOTES = `### Added
- Crash reports carry the session log; main-thread hangs are reported as "Not responding" by @Horuse in (#70).

### Changed
- Spectrum FFT now runs in Rust and bars render via canvas, cutting main-thread and IPC load by @a-32x in (#62).

### Fixed
- Fixed the window freezing on Windows when opening or starting a pipeline by @Horuse in (#71).

**Full Changelog**: https://github.com/Horuse/Splitwave/compare/v1.4.1...v1.5.0`;

	const STATES: Record<string, UpdateState> = {
		available: { phase: 'available', update, notes: NOTES },
		downloading: { phase: 'downloading', update, downloaded: 12_400_000, total: 31_000_000 },
		upToDate: { phase: 'up_to_date' },
		error: { phase: 'error', message: 'error sending request for url (https://example.invalid/latest.json)' }
	};
</script>

<Story name="Available" asChild>
	<StoreScope apply={() => (updaterStore.state = STATES.available)}><UpdateBanner /></StoreScope>
</Story>
<Story name="Downloading" asChild>
	<StoreScope apply={() => (updaterStore.state = STATES.downloading)}><UpdateBanner /></StoreScope>
</Story>
<Story name="Up to date" asChild>
	<StoreScope apply={() => (updaterStore.state = STATES.upToDate)}><UpdateBanner /></StoreScope>
</Story>
<Story name="Error" asChild>
	<StoreScope apply={() => (updaterStore.state = STATES.error)}><UpdateBanner /></StoreScope>
</Story>
