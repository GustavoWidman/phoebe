<!-- the desktop sidebar: the fleet, the logs, then every alert, machine and service -->
<script lang="ts">
	import { dur, pct } from '#lib/derive/format.ts';
	import {
		TREATMENT,
		alertDot,
		alertTitle,
		firingOf,
		machineDot,
		serviceStatus
	} from '#lib/derive/status.ts';
	import { href, type Route } from '#lib/router.svelte.ts';
	import Dot from '#lib/ui/Dot.svelte';
	import Icon from '#lib/ui/Icon.svelte';
	import Label from '#lib/ui/Label.svelte';
	import Pips from '#lib/ui/Pips.svelte';
	import type { Snapshot } from '#lib/wire/types.ts';
	import Account from './Account.svelte';

	let { route, snap }: { route: Route; snap: Snapshot } = $props();
	const firing = $derived(firingOf(snap));
	const is = (screen: Route['screen'], key?: string) =>
		route.screen === screen &&
		(key === undefined || ('name' in route ? route.name : 'id' in route ? route.id : '') === key);
</script>

<div class="side-h"><span class="wm"><Icon name="moon" />phoebe</span><Account /></div>
<a class="si" class:on={is('fleet')} href={href({ screen: 'fleet' })}>
	<Dot s={firing.length ? 'bad' : 'ok'} /><span class="ell">Fleet</span><small>
		{firing.length ? `${firing.length} firing` : 'all clear'}
	</small>
</a>
<a class="si" class:on={is('logs')} href={href({ screen: 'logs' })}>
	<Dot s="ok" /><span class="ell">Logs</span><small>live</small>
</a>
<Label label="Alerts" value={firing.length || ''} tone={firing.length ? 'bad' : ''} />
{#each snap.alerts as a (a.id)}
	{@const live = a.state.kind === 'firing'}
	<a
		class="si"
		class:on={is('alert', a.id)}
		class:sev={live}
		class:sev-warn={live && TREATMENT[a.severity] === 'warn'}
		href={href({ screen: 'alert', id: a.id })}
	>
		<Dot s={alertDot(a)} pulse={live && TREATMENT[a.severity] === 'crit'} /><span class="ell">
			{alertTitle(a)}
		</span><small>{live ? dur(snap.at - a.starts_at) : 'resolved'}</small>
	</a>
{/each}
<Label label="Machines" />
{#each snap.machines as m (m.name)}
	<a class="si" class:on={is('machine', m.name)} href={href({ screen: 'machine', name: m.name })}>
		<Dot s={machineDot(m)} /><span class="ell m">{m.name}</span>
		{#if m.state.kind === 'up' && m.state.units.failed.length}
			<small class="bad">{m.state.units.failed.length} failed</small>
		{:else if m.state.kind === 'up'}
			<small>{pct(m.state.vitals.cpu)}</small>
		{:else}
			<small>{m.state.kind}</small>
		{/if}
	</a>
{/each}
<Label label="Services" />
{#each snap.services as s (s.name)}
	{@const st = serviceStatus(s, snap.nodes)}
	<a class="si" class:on={is('service', s.name)} href={href({ screen: 'service', name: s.name })}>
		<Dot s={st.dot} /><span class="ell m">{s.name}</span>
		{#if st.reached}
			<Pips dots={st.reach.map((r) => (r.ok === false ? 'bad' : r.ok ? 'ok' : 'idle'))} />
		{:else}
			<small class:bad={s.unit?.state === 'failed'}>{s.unit?.state ?? ''}</small>
		{/if}
	</a>
{/each}
