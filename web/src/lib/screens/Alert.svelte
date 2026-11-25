<!--
	An alert, where a notification lands: the severity header across the full width, which
	nodes see it, its timeline, its details, the series that shows it, its logs, and the other
	alerts. A resolved alert keeps the same screen without the severity.
-->
<script lang="ts">
	import { Load, alertUrl } from '#lib/api.svelte.ts';
	import AlertRow from '#lib/alert/AlertRow.svelte';
	import Band from '#lib/alert/Band.svelte';
	import Hero from '#lib/alert/Hero.svelte';
	import On from '#lib/alert/On.svelte';
	import { date, hms, latency, load, pct, rate } from '#lib/derive/format.ts';
	import { alertDot } from '#lib/derive/status.ts';
	import { events, firedAt } from '#lib/derive/timeline.ts';
	import { LogFeed } from '#lib/log/feed.svelte.ts';
	import Chart from '#lib/ui/Chart.svelte';
	import Dot from '#lib/ui/Dot.svelte';
	import Label from '#lib/ui/Label.svelte';
	import Lines from '#lib/ui/Lines.svelte';
	import { layout } from '#lib/ui/layout.svelte.ts';
	import { parseAlertDetail } from '#lib/wire/parse.ts';
	import type { Alert, AlertDetail, Metric, Snapshot } from '#lib/wire/types.ts';

	let { snap, id }: { snap: Snapshot; id: string } = $props();

	const detail = new Load<AlertDetail>(
		() => alertUrl(id),
		parseAlertDetail,
		() => snap.at
	);
	const alert = $derived<Alert | null>(
		snap.alerts.find((a) => a.id === id) ?? detail.value?.alert ?? null
	);
	const timeline = $derived(detail.value?.timeline ?? []);
	const fired = $derived(firedAt(timeline));
	const notified = $derived(timeline.find((e) => e.kind === 'notified' && !e.resolved));
	const resolved = $derived(alert?.state.kind === 'resolved');
	const others = $derived(snap.alerts.filter((a) => a.id !== id));
	const logs = new LogFeed(
		() =>
			layout.desk || !alert
				? null
				: alert.service
					? { service: alert.service }
					: alert.machine
						? { machine: alert.machine }
						: {},
		() => false,
		12
	);

	const METRIC: Record<Metric, { name: string; fmt: (v: number | null) => string; max?: number }> =
		{
			cpu: { name: 'CPU', fmt: pct, max: 1 },
			mem: { name: 'Memory', fmt: pct, max: 1 },
			disk: { name: 'Disk /', fmt: pct, max: 1 },
			net: { name: 'Network', fmt: rate },
			load: { name: 'Load', fmt: load },
			latency: { name: 'Latency', fmt: latency }
		};
	const series = $derived(detail.value?.series ?? null);
	const subject = $derived(
		series ? ('machine' in series.of ? series.of.machine : series.of.service) : ''
	);
</script>

{#snippet seenBy(a: Alert)}
	<Label label="Seen by" value="{a.seen_by.length} of {snap.nodes.length} nodes" />
	{#each snap.nodes as n (n.name)}
		{@const seen = a.seen_by.includes(n.name)}
		{@const at = fired.get(n.name)}
		<div class="row" data-node={n.name}>
			<Dot s={seen ? (resolved ? 'off' : alertDot(a)) : 'ok'} />
			<div><span class="nm m">{n.name}</span></div>
			<span class="rt">
				{seen ? (at ? `${resolved ? 'saw it' : 'since'} ${hms(at)}` : 'sees it') : 'not firing'}
			</span>
		</div>
	{/each}
{/snippet}

{#snippet told()}
	<Label label="Timeline" />
	<ul class="tl">
		{#each events(timeline, snap.nodes.length) as e, i (i)}
			<li><time>{hms(e.at)}</time><Dot s={e.dot} /><span>{e.text}</span></li>
		{/each}
	</ul>
{/snippet}

{#snippet details(a: Alert)}
	<Label label="Details" />
	<dl class="kv">
		<div>
			<dt>Rule</dt>
			<dd class="m">{a.rule} · {a.severity}</dd>
		</div>
		<div>
			<dt>On</dt>
			<dd><On alert={a} links bare /></dd>
		</div>
		<div>
			<dt>Started</dt>
			<dd class="m">{date(a.starts_at)} · {hms(a.starts_at)}</dd>
		</div>
		{#if a.state.kind === 'resolved'}<div>
				<dt>Ended</dt>
				<dd class="m">{date(a.state.at)} · {hms(a.state.at)}</dd>
			</div>{/if}
		<div>
			<dt>Notified</dt>
			<dd class="m">
				{notified
					? `${hms(notified.at)} · by ${notified.kind === 'notified' ? notified.by : ''}`
					: a.severity === 'page'
						? 'not yet'
						: 'no push: info stays in the app'}
			</dd>
		</div>
	</dl>
{/snippet}

{#if alert}
	<Hero {alert} now={snap.at} />
	{#if layout.desk}
		<div class="two">
			<div>{@render told()}</div>
			<div>{@render seenBy(alert)}{@render details(alert)}</div>
		</div>
	{:else}
		{@render seenBy(alert)}
		{@render told()}
		{@render details(alert)}
	{/if}
	{#if series}
		{@const m = METRIC[series.metric]}
		<Label label="{m.name}, {subject}" value="dashed line: fired" />
		<Chart
			lines={series.data.kind === 'lines' ? series.data.lines : []}
			band={series.data.kind === 'band' ? series.data.points : undefined}
			from={series.from}
			to={series.to}
			now={snap.at}
			fmt={m.fmt}
			max={m.max}
			area
			mark={alert.starts_at}
			h={100}
		/>
	{/if}
	{#if !layout.desk}
		<Label label="Logs" value="newest" />
		<Lines lines={logs.lines} />
	{/if}
	{#if others.length}
		<Label label="Other alerts" />
		{#each others as a (a.id)}
			{#if a.state.kind === 'firing'}<Band
					alert={a}
					nodes={snap.nodes}
					now={snap.at}
				/>{:else}<AlertRow alert={a} now={snap.at} />{/if}
		{/each}
	{/if}
{:else if detail.failed}
	<div class="tb">
		<div class="k">Alert</div>
		<h1>Not found</h1>
		<p class="sub">No node knows this alert any more. It may have resolved more than a week ago.</p>
	</div>
{/if}
