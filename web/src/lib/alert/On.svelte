<!-- what an alert is on: its machine, and its service or unit; links when `links` is set -->
<script lang="ts">
	import { href } from '#lib/router.svelte.ts';
	import type { Alert } from '#lib/wire/types.ts';

	let { alert, links = false }: { alert: Alert; links?: boolean } = $props();
	const unit = $derived(alert.labels.name);
</script>

{#if alert.machine}on {#if links}<a
			class="m ul"
			href={href({ screen: 'machine', name: alert.machine })}
		>
			{alert.machine}
		</a>{:else}<span class="m">{alert.machine}</span>{/if}{/if}{#if alert.service}
	· {#if links}<a class="m ul" href={href({ screen: 'service', name: alert.service })}>
			{unit ?? alert.service}
		</a>{:else}<span class="m">{unit ?? alert.service}</span>{/if}{:else if unit}
	· <span class="m">{unit}</span>{/if}
