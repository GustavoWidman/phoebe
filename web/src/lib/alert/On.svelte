<!--
	What an alert is on: its machine, then its unit or service, as links when `links` is set.
	`bare` leaves out the leading "on", for a field already labeled On.
-->
<script lang="ts">
	import { href, type Route } from '#lib/router.svelte.ts';
	import type { Alert } from '#lib/wire/types.ts';

	let {
		alert,
		links = false,
		bare = false
	}: { alert: Alert; links?: boolean; bare?: boolean } = $props();

	const parts = $derived(
		[
			alert.machine && {
				text: alert.machine,
				to: { screen: 'machine', name: alert.machine } satisfies Route
			},
			(alert.labels.name || alert.service) && {
				text: alert.labels.name ?? alert.service!.name,
				to: alert.service ? ({ screen: 'service', ...alert.service } satisfies Route) : null
			}
		].filter((p) => !!p)
	);
</script>

{#if parts.length && !bare}{'on '}{/if}{#each parts as p, i (i)}{#if i}{' · '}{/if}{#if links && p.to}<a
			class="m ul"
			href={href(p.to)}
		>
			{p.text}
		</a>{:else}<span class="m">{p.text}</span>{/if}{/each}
