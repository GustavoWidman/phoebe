<!-- an alert as an ordinary row: a resolved one, or one listed among others -->
<script lang="ts">
	import { dur, hm } from '#lib/derive/format.ts';
	import { alertDot, alertTitle } from '#lib/derive/status.ts';
	import { href } from '#lib/router.svelte.ts';
	import Dot from '#lib/ui/Dot.svelte';
	import Icon from '#lib/ui/Icon.svelte';
	import type { Alert } from '#lib/wire/types.ts';

	let { alert, now }: { alert: Alert; now: number } = $props();
</script>

<a class="row" href={href({ screen: 'alert', id: alert.id })} data-alert={alert.id}>
	<Dot s={alertDot(alert)} />
	<div>
		<div class="rh"><span class="nm ell">{alertTitle(alert)}</span></div>
		<div class="sb" class:mute={alert.state.kind === 'resolved'}>
			{#if alert.state.kind === 'resolved'}
				resolved {hm(alert.state.at)} after {dur(alert.state.at - alert.starts_at)}
			{:else}
				{alert.rule} · {dur(now - alert.starts_at)}
			{/if}
		</div>
	</div>
	<span class="rt"><Icon name="chev" /></span>
</a>
