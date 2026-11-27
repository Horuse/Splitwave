<script lang="ts">
	import { audioStore } from '../stores.svelte';
	import ElapsedTime from './elapsed_time.svelte';
</script>

<ElapsedTime since={audioStore.startedAt} />
