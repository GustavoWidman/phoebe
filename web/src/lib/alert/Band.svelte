<!-- the severity treatment at band scale: one firing alert as a row of the fleet -->
<script lang="ts">
	import { dur } from '#lib/derive/format.ts';
	import { TREATMENT, alertDot, alertTitle } from '#lib/derive/status.ts';
	import { href } from '#lib/router.svelte.ts';
	import Icon from '#lib/ui/Icon.svelte';
	import Pips from '#lib/ui/Pips.svelte';
	import type { Alert, Node } from '#lib/wire/types.ts';
	import Kicker from './Kicker.svelte';
	import On from './On.svelte';

	let { alert, nodes, now }: { alert: Alert; nodes: Node[]; now: number } = $props();
</script>

<a
	class="band sev"
	class:sev-warn={TREATMENT[alert.severity] === 'warn'}
	href={href({ screen: 'alert', id: alert.id })}
	data-alert={alert.id}
>
	<Kicker {alert} age={dur(now - alert.starts_at)} />
	<div class="tt">{alertTitle(alert)}</div>
	<div class="mt"><On {alert} /></div>
	<div class="ft">
		<Pips dots={nodes.map((n) => (alert.seen_by.includes(n.name) ? alertDot(alert) : 'idle'))} />
		seen by {alert.seen_by.length} of {nodes.length} nodes<Icon name="chev" />
	</div>
</a>
