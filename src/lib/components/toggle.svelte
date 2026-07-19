<script lang="ts">
	interface Props {
		checked: boolean;
		onChange: (checked: boolean) => void;
		label?: string;
		hint?: string;
		disabled?: boolean;
		/** `sm` fits inside a node; the default suits a settings row. */
		size?: 'sm' | 'md';
	}

	let { checked, onChange, label, hint, disabled = false, size = 'md' }: Props = $props();

	let track = $derived(size === 'sm' ? 'h-3.5 w-6' : 'h-5 w-9');
	let knob = $derived(size === 'sm' ? 'size-2.5' : 'size-4');
	let offset = $derived(
		size === 'sm'
			? checked
				? 'left-3'
				: 'left-0.5'
			: checked
				? 'left-4.5'
				: 'left-0.5'
	);
</script>

<button
	type="button"
	role="switch"
	aria-checked={checked}
	aria-label={label}
	{disabled}
	onclick={() => onChange(!checked)}
	class={[
		'nodrag nopan flex items-center gap-2 text-left disabled:opacity-40',
		hint && 'w-full justify-between rounded-xl border border-neutral-400 bg-neutral-100 p-3 hover:bg-neutral-200'
	]}
>
	{#if label}
		<span class="flex flex-col">
			<span class={hint ? 'text-xs font-medium text-theme' : 'text-[10px] text-neutral-1000'}>
				{label}
			</span>
			{#if hint}
				<span class="text-[11px] text-neutral-900">{hint}</span>
			{/if}
		</span>
	{/if}
	<span
		class={[
			'relative shrink-0 rounded-full transition-colors',
			track,
			checked ? 'bg-neutral-900' : 'bg-neutral-400'
		]}
	>
		<span
			class={['absolute top-0.5 rounded-full bg-background transition-all', knob, offset]}
		></span>
	</span>
</button>
