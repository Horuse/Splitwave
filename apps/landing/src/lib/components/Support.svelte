<script lang="ts">
	import type { Component } from 'svelte';
	import * as Icons from '#lib/components/icons/index.js';

	type Wallet = {
		id: string;
		label: string;
		network: string;
		address: string;
		icon: Component<{ class?: string }>;
		networkIcon?: Component<{ class?: string }>;
	};

	const wallets: Wallet[] = [
		{
			id: 'usdt',
			label: 'Tether USDT',
			network: 'TRC20',
			address: 'TLhTvnn8CtVuQZruLXmRurGhR9GWd7DrWZ',
			icon: Icons.Tether,
			networkIcon: Icons.Tron
		},
		{
			id: 'ton',
			label: 'TON',
			network: 'TON',
			address: 'UQCpokpaZfwmVTjKDj0LrAbEPO-65c81-MiuBQOa7lTXbMGR',
			icon: Icons.Ton
		},
		{
			id: 'btc',
			label: 'Bitcoin',
			network: 'BTC',
			address: 'bc1q6tusr5rht7dgmw8gqzkx7rwdg4q8932lwn2rsy',
			icon: Icons.Bitcoin
		}
	];

	let copied = $state('');

	async function copyAddress(id: string, address: string) {
		try {
			await navigator.clipboard.writeText(address);
			copied = id;
			setTimeout(() => (copied = ''), 2000);
		} catch {
			copied = '';
		}
	}
</script>

<div class="flex flex-col gap-5 rounded-xl border border-dashed border-neutral-400 p-4">
	<div class="flex flex-col gap-2">
		<div class="flex items-center gap-2">
			<Icons.Patreon class="size-6 shrink-0 text-[#FF424D]" />
			<span class="text-sm font-semibold text-theme">Patreon</span>
			<span class="text-xs text-neutral-900">Membership</span>
		</div>
		<div
			class="flex items-center justify-between gap-3 rounded-xl border border-neutral-200 bg-neutral-50 p-2 pl-3"
		>
			<span class="truncate text-xs text-neutral-1000">patreon.com/splitwave</span>
			<a
				href="https://www.patreon.com/splitwave/membership"
				target="_blank"
				rel="noopener noreferrer"
				class="btn-preset inline-flex items-center gap-1.5 !px-3 !py-1.5 !text-xs"
			>
				<span>Support</span>
				<Icons.Open class="size-3.5" />
			</a>
		</div>
	</div>

	{#each wallets as wallet (wallet.id)}
		{@const Icon = wallet.icon}
		<div class="flex flex-col gap-2">
			<div class="flex items-center gap-2">
				<Icon class="size-6 shrink-0" />
				<span class="text-sm font-semibold text-theme">{wallet.label}</span>
				<span class="flex items-center gap-1 text-xs text-neutral-900">
					{#if wallet.networkIcon}
						{@const NetworkIcon = wallet.networkIcon}
						<NetworkIcon class="size-3.5 shrink-0" />
					{/if}
					{wallet.network}
				</span>
			</div>
			<div
				class="flex flex-wrap items-center gap-2 rounded-xl border border-neutral-200 bg-neutral-50 p-1 pl-3"
			>
				<code class="flex-1 font-mono text-xs break-all text-theme select-all">
					{wallet.address}
				</code>
				<button
					type="button"
					class="btn-preset !px-3 !py-1.5 !text-xs"
					onclick={() => copyAddress(wallet.id, wallet.address)}
				>
					{copied === wallet.id ? 'Copied' : 'Copy'}
				</button>
			</div>
		</div>
	{/each}
</div>
