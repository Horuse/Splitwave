<script lang="ts">
	import type { Announcement } from '$lib/modules/announcements';

	let { announcement }: { announcement: Announcement } = $props();

	const TONE = {
		critical: 'bg-red-500/15 text-red-500',
		warning: 'bg-amber-500/15 text-amber-500',
		success: 'bg-emerald-500/15 text-emerald-500',
		info: 'bg-sky-500/15 text-sky-500'
	} as const;
</script>

<span class={['rounded-md px-2 py-0.5 text-[10px] font-semibold tracking-wide uppercase', TONE[announcement.severity]]}>
	{announcement.badge ?? announcement.severity}
</span>
