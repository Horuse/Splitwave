<script lang="ts">
	const GITHUB = 'https://github.com/Horuse/Splitwave';

	import * as Icons from '#lib/components/icons/index.js';
</script>

<header class="sticky top-0 z-50 border-b border-neutral-200 bg-background/80 backdrop-blur-md">
	<div
		class="flex max-w-4xl flex-col items-center justify-between gap-3 px-3 py-3 xs:mx-auto sm:flex-row sm:px-6"
	>
		<div class="flex w-full justify-between">
			<a href="/" aria-label="Splitwave — main page" class="flex items-center gap-2.5">
				<img src="/icon.svg" alt="Splitwave" title="Splitwave" class="size-7 rounded-lg" />
				<span class="text-sm font-semibold tracking-wide">Splitwave</span>
			</a>
			<a
				href={GITHUB}
				target="_blank"
				rel="noopener noreferrer"
				class="btn-secondary px-3 py-1.5 text-xs sm:hidden"
			>
				GitHub
				<Icons.ArrowUpRight class="size-3" />
			</a>
		</div>
		<div class="my-3 flex w-auto shrink-0 flex-col items-center gap-3 xs:my-0 xs:flex-row">
			<div class="flex items-center gap-4">
				<a href="/download" class="text-xs text-neutral-1000 transition-colors hover:text-theme"
					>Download</a
				>
				<a href="/wiki" class="text-xs text-neutral-1000 transition-colors hover:text-theme">Wiki</a>
				<a href="/changelog" class="text-xs text-neutral-1000 transition-colors hover:text-theme"
					>Release notes</a
				>
			</div>
			<span
				class="rounded-full border border-neutral-300 bg-neutral-200 px-2.5 py-0.5 text-xs whitespace-nowrap text-neutral-1000"
				>macOS · Linux · Windows</span
			>
			<a
				href={GITHUB}
				target="_blank"
				rel="noopener noreferrer"
				class="btn-secondary hidden px-3 py-1.5 text-xs! sm:flex"
			>
				GitHub
				<Icons.ArrowUpRight class="size-3" />
			</a>
		</div>
	</div>
</header>
