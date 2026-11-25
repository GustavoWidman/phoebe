<!-- FIRING · PAGE · UNITFAILED, in the severity's ink, with the age on the right -->
<script lang="ts">
	import { TREATMENT, alertDot } from '#lib/derive/status.ts';
	import Dot from '#lib/ui/Dot.svelte';
	import type { Alert } from '#lib/wire/types.ts';

	let { alert, age = '' }: { alert: Alert; age?: string } = $props();
	const firing = $derived(alert.state.kind === 'firing');
</script>

<div class="sk">
	<Dot s={alertDot(alert)} pulse={firing && TREATMENT[alert.severity] === 'crit'} />
	{firing ? 'Firing' : 'Resolved'} · {alert.severity} · {alert.rule}
	{#if age}<span class="ag">{age}</span>{/if}
</div>
