<script lang="ts">
	type Props = {
		checked: boolean;
		onChange: (checked: boolean) => void;
		label?: string;
		hint?: string;
		disabled?: boolean;
		size?: 'sm' | 'md';
		class?: string;
	};

	let { checked, onChange, label, hint, disabled = false, size = 'md', class: className }: Props = $props();
</script>

<button
	type="button"
	role="switch"
	aria-checked={checked}
	aria-label={label}
	{disabled}
	onclick={() => onChange(!checked)}
	class={['toggle', size === 'sm' && 'toggle-sm', hint && 'toggle-row', className]}>
	{#if label}
		<span class="flex flex-col">
			<span class="toggle-label">{label}</span>
			{#if hint}
				<span class="toggle-hint">{hint}</span>
			{/if}
		</span>
	{/if}
	<span class="toggle-track" data-on={checked}>
		<span class="toggle-knob"></span>
	</span>
</button>
