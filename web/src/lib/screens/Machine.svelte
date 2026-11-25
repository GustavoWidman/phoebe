<!--
	A machine: what it is, its firing alerts, cpu, memory, disk, network and load over a range,
	its units, and its recent logs. A desktop at rest shows only that it rests.
-->
<script lang="ts">
	import { Load, RANGES, seriesUrl, type Range } from '#lib/api.svelte.ts';
	import Band from '#lib/alert/Band.svelte';
	import { dur, gib, hm, load, pct, rate, when } from '#lib/derive/format.ts';
	import { alertsOn, machineDot, presence } from '#lib/derive/status.ts';
	import { LogFeed } from '#lib/log/feed.svelte.ts';
	import { href } from '#lib/router.svelte.ts';
	import Chart from '#lib/ui/Chart.svelte';
	import Choices from '#lib/ui/Choices.svelte';
	import Dot from '#lib/ui/Dot.svelte';
	import Label from '#lib/ui/Label.svelte';
	import Lines from '#lib/ui/Lines.svelte';
	import { layout } from '#lib/ui/layout.svelte.ts';
	import { parseSeries } from '#lib/wire/parse.ts';
	import type { Machine, Metric, Series, Snapshot } from '#lib/wire/types.ts';

	let { snap, machine: m }: { snap: Snapshot; machine: Machine } = $props();

	let range = $state<Range>('6h');
	const minute = () => Math.floor(snap.at / 60_000);
	const resting = $derived(m.state.kind === 'resting');
	const metric = (k: Metric) =>
		new Load(
			() => (resting ? null : seriesUrl({ machine: m.name }, k, range)),
			parseSeries,
			minute
		);
	const series = {
		cpu: metric('cpu'),
		mem: metric('mem'),
		disk: metric('disk'),
		net: metric('net'),
		load: metric('load')
	};

	const alerts = $derived(alertsOn(snap, { machine: m.name }));
	const mark = $derived(alerts[0]?.starts_at ?? null);
	const services = $derived(snap.services.filter((s) => s.machine === m.name && s.unit));
	const failed = $derived(m.state.kind === 'up' ? m.state.units.failed : []);
	const active = $derived(m.state.kind === 'up' ? m.state.units.active : 0);
	const recent = new LogFeed(
		() => (layout.desk ? null : { machine: m.name }),
		() => false,
		8
	);

	const lines = (s: Series | null) => (s?.data.kind === 'lines' ? s.data.lines : []);
	const last = (s: Series | null, i = 0) =>
		lines(s)[i]?.points.findLast((p) => p[1] != null)?.[1] ?? null;
	const cores = $derived(m.hardware?.cores ?? 1);
	const loadMax = $derived(
		Math.max(
			cores,
			...lines(series.load.value).flatMap((l) => l.points.map((p) => (p[1] ?? 0) * 1.2))
		)
	);
	const charts = $derived([
		{ name: 'CPU', s: series.cpu.value, now: pct(last(series.cpu.value)), fmt: pct, max: 1 },
		{ name: 'Memory', s: series.mem.value, now: pct(last(series.mem.value)), fmt: pct, max: 1 },
		{ name: 'Disk /', s: series.disk.value, now: pct(last(series.disk.value)), fmt: pct, max: 1 },
		{
			name: 'Network',
			s: series.net.value,
			now: `↓${rate(last(series.net.value, 0))} ↑${rate(last(series.net.value, 1))}`,
			fmt: rate,
			max: undefined
		},
		{
			name: 'Load',
			s: series.load.value,
			now: load(last(series.load.value)),
			fmt: load,
			max: loadMax
		}
	]);
</script>

<div class="tb">
	<div class="k">Machine · {m.os} {m.always_on ? 'server' : 'desktop'}</div>
	<h1 class="m">{m.name}</h1>
	<div class="meta">
		{#if m.hardware}
			<span><b>{m.hardware.arch}</b></span>
			<span><b>{m.hardware.cores}</b> cores · <b>{gib(m.hardware.mem_bytes)}</b></span>
			<span>disk <b>{gib(m.hardware.disk_bytes)}</b></span>
		{/if}
		{#if m.state.kind === 'up'}<span>up <b>{dur(snap.at - m.state.booted_at)}</b></span>{/if}
	</div>
	{#if m.state.kind !== 'up'}
		<div class="st">
			<Dot s={machineDot(m)} />{presence(m, snap.at)}.{resting
				? ' Not an alert: desktops rest.'
				: ''}
		</div>
	{/if}
</div>
{#if alerts.length}
	<div class="bands">
		{#each alerts as a (a.id)}<Band alert={a} nodes={snap.nodes} now={snap.at} />{/each}
	</div>
{/if}

{#if resting}
	<Label label="Last report" value={when(m.state.since, snap.at)} />
	<p class="sub">No metrics while it rests. phoebe keeps its last state and stays quiet.</p>
{:else}
	<Choices options={RANGES} on={range} pick={(r) => (range = r)} />
	<div class="charts" class:two-up={layout.desk}>
		{#each charts as c (c.name)}
			<div data-chart={c.name}>
				<Label label={c.name} value={c.s ? c.now : ''} />
				{#if c.s}
					<Chart
						lines={lines(c.s)}
						from={c.s.from}
						to={c.s.to}
						now={snap.at}
						max={c.max}
						fmt={c.fmt}
						area={lines(c.s).length === 1}
						{mark}
						h={88}
						days={range === '7d'}
					/>
				{:else}
					<div class="ch" style:--h="88px"></div>
				{/if}
			</div>
		{/each}
	</div>

	<Label
		label="Units"
		value="{active} active{failed.length ? ` · ${failed.length} failed` : ''}"
		tone={failed.length ? 'bad' : ''}
	/>
	{#if layout.desk}
		<table class="tbl">
			<thead><tr><th></th><th>unit</th><th>service</th><th class="n">state</th></tr></thead>
			<tbody>
				{#each failed as u (u.name)}
					<tr>
						<td><Dot s="bad" /></td><td class="m nm">{u.name}</td>
						<td class="bad">failed {when(u.since, snap.at)}</td><td class="n m bad">failed</td>
					</tr>
				{/each}
				{#each services.filter((s) => s.unit?.state !== 'failed') as s (s.name)}
					<tr data-go>
						<td><Dot s={s.unit?.state === 'active' ? 'ok' : 'idle'} /></td>
						<td class="m nm">{s.unit?.name}</td>
						<td><a class="ul" href={href({ screen: 'service', name: s.name })}>{s.name}</a></td>
						<td class="n m">{s.unit?.state}</td>
					</tr>
				{/each}
			</tbody>
		</table>
	{:else}
		{#each failed as u (u.name)}
			<div class="row">
				<Dot s="bad" />
				<div>
					<div class="rh"><span class="nm m ell">{u.name}</span></div>
					<div class="sb bad">failed {when(u.since, snap.at)}</div>
				</div>
				<span class="rt"><span class="bad">failed</span></span>
			</div>
		{/each}
		{#each services.filter((s) => s.unit?.state !== 'failed') as s (s.name)}
			<a class="row" href={href({ screen: 'service', name: s.name })}>
				<Dot s={s.unit?.state === 'active' ? 'ok' : 'idle'} />
				<div>
					<div class="rh"><span class="nm m ell">{s.unit?.name}</span></div>
					<div class="sb mute">
						{s.name}{s.unit?.since ? ` · since ${when(s.unit.since, snap.at)}` : ''}
					</div>
				</div>
				<span class="rt">{s.unit?.state}</span>
			</a>
		{/each}
		<Label label="Recent logs" value="all in the sheet" />
		<Lines lines={recent.lines} />
	{/if}
{/if}
