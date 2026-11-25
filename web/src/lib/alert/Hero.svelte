<!-- the severity treatment at hero scale: the alert screen's header -->
<script lang="ts">
	import { TREATMENT, alertProse, alertSpan, alertTitle } from '#lib/derive/status.ts';
	import type { Alert } from '#lib/wire/types.ts';
	import Kicker from './Kicker.svelte';
	import On from './On.svelte';

	let { alert, now }: { alert: Alert; now: number } = $props();
	const firing = $derived(alert.state.kind === 'firing');
</script>

<header
	class="hero"
	class:sev={firing}
	class:sev-warn={firing && TREATMENT[alert.severity] === 'warn'}
	class:res={!firing}
>
	<Kicker {alert} />
	<h1>{alertTitle(alert)}</h1>
	<div class="hm"><On {alert} links /></div>
	<div class="hm">{alertSpan(alert, now)}</div>
	<p>{alertProse(alert)}</p>
</header>
