<script lang="ts">
	const GITHUB = 'https://github.com/Horuse/Splitwave';
</script>

<footer class="border-t border-neutral-200">
	<div
		class="mx-auto flex max-w-4xl flex-col-reverse items-center justify-between gap-8 px-6 py-5 text-xs text-neutral-900 sm:flex-row"
	>
		<div class="flex flex-col items-center gap-1 sm:items-start">
			<span class="whitespace-nowrap">Splitwave © 2026</span>
			<span class="text-center sm:text-left"
				>Other product names are trademarks of their respective owners.</span
			>
		</div>
		<div class="flex flex-wrap items-center justify-center gap-4 sm:justify-end">
			<a href="/download" class="transition-colors hover:text-theme">Download</a>
			<a href="/changelog" class="transition-colors hover:text-theme">Release history</a>
			<a
				href="https://www.patreon.com/splitwave/membership"
				target="_blank"
				rel="noopener noreferrer"
				class="transition-colors hover:text-theme">Patreon</a
			>
			<a href="/terms" class="transition-colors hover:text-theme">Terms</a>
			<a href="/privacy" class="transition-colors hover:text-theme">Privacy</a>
			<a href="mailto:support@splitwave.app" class="transition-colors hover:text-theme"
				>support@splitwave.app</a
			>
			<a
				href={GITHUB}
				target="_blank"
				rel="noopener noreferrer"
				class="transition-colors hover:text-theme">MIT License</a
			>
		</div>
	</div>
</footer>
