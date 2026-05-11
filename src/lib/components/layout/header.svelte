<script lang="ts">
	import { themeStore } from '$lib/stores/theme.ts';

	let { left, right } = $props();
</script>

<header
	data-tauri-drag-region
	class="flex h-10 w-full flex-row gap-8 pl-20 items-center border-b border-theme/5 bg-background px-1.5"
>
	{@render left?.()}

    <div class="ml-auto flex items-center flex-row gap-4">
        {@render right?.()}

        <button
            class="button-header size-7"
            aria-label="Theme toggle"
            onclick={() => ($themeStore == 'light' ? ($themeStore = 'dark') : ($themeStore = 'light'))}
        >
            <svg
                class="size-4 fill-current"
                fill="none"
                viewBox="0 0 24 24"
                xmlns="http://www.w3.org/2000/svg"
                ><path
                    d="M12 22c5.523 0 10-4.477 10-10S17.523 2 12 2 2 6.477 2 12s4.477 10 10 10Zm0-2V4a8 8 0 1 1 0 16Z"
                /></svg
            >
        </button>
    </div>
</header>
