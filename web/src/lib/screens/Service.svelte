<!--
	A service: who reaches it on the probe map, where tapping a node shows that node's latency,
	a row per node, the latency band across all of them, its unit, and its recent logs.
-->
<script lang="ts">
	import { untrack } from 'svelte';
	import { Load, RANGES, seriesUrl, type Range } from '#lib/api.svelte.ts';
	import Band from '#lib/alert/Band.svelte';
	import { latency, when } from '#lib/derive/format.ts';
	import { alertsOn, hostOf, median, serviceStatus, timesOut } from '#lib/derive/status.ts';
	import { LogFeed } from '#lib/log/feed.svelte.ts';
	import { href } from '#lib/router.svelte.ts';
	import Chart from '#lib/ui/Chart.svelte';
	import Choices from '#lib/ui/Choices.svelte';
	import Dot from '#lib/ui/Dot.svelte';
	import Label from '#lib/ui/Label.svelte';
	import Lines from '#lib/ui/Lines.svelte';
	import ProbeMap from '#lib/ui/ProbeMap.svelte';
	import { gaps } from '#lib/ui/plot.ts';
	import Spark from '#lib/ui/Spark.svelte';
	import { layout } from '#lib/ui/layout.svelte.ts';
	import { parseSeries } from '#lib/wire/parse.ts';
	import type { Series, Service, Snapshot } from '#lib/wire/types.ts';

	let { snap, service: s }: { snap: Snapshot; service: Service } = $props();

	const st = $derived(serviceStatus(s, snap.nodes));
	const alerts = $derived(alertsOn(snap, { service: s.name }));
	const probed = $derived(s.probes.length > 0);
	// the first node that cannot reach it, else the first node
	const first = untrack(() => serviceStatus(s, snap.nodes));
	let selected = $state(first.failing[0]?.node ?? first.reach[0]?.node ?? '');
	let range = $state<Range>('6h');
	const minute = () => Math.floor(snap.at / 60_000);

	const band = new Load(
		() => (probed ? seriesUrl({ service: s.name }, 'latency', range) : null),
		parseSeries,
		minute
	);
	const perNode = new Map(
		first.reach.map((n) => [
			n.node,
			new Load(
				() => (probed ? seriesUrl({ service: s.name }, 'latency', range, n.node) : null),
				parseSeries,
				minute
			)
		])
	);
	const recent = new LogFeed(
		() => (layout.desk ? null : { service: s.name }),
		() => false,
		8
	);

	const points = (x: Series | null | undefined) =>
		x?.data.kind === 'lines' ? (x.data.lines[0]?.points ?? []) : [];
	const hosts = $derived([...new Set(s.probes.map((p) => hostOf(p.target)))].join(', '));
	const sel = $derived(st.reach.find((r) => r.node === selected));
	const selSeries = $derived(perNode.get(selected)?.value ?? null);
	const kinds = $derived([
		...new Set(s.probes.map((p) => (p.kind === 'http' && p.tls ? 'https' : p.kind)))
	]);
	const stats = (x: Series | null | undefined) => {
		const v = points(x).flatMap((p) => (p[1] == null ? [] : [p[1]]));
		return { p50: median(v), max: v.length ? Math.max(...v) : null };
	};
	const result = (ok: boolean | null) =>
		ok === null
			? 'not probed'
			: ok
				? `${s.probes.length === 1 ? 'probe' : `${s.probes.length} probes`} ok`
				: 'timeout';
	const unitDot = $derived(
		s.unit?.state === 'active'
			? 'ok'
			: s.unit?.state === 'failed'
				? 'bad'
				: s.unit?.state === 'activating'
					? 'acc'
					: 'idle'
	);
</script>

<div class="tb">
	<div class="k">
		Service · {probed
			? `${kinds.join(', ')} probe${s.probes.length === 1 ? '' : 's'}`
			: 'no probes'}
	</div>
	<h1 class="m">{s.name}</h1>
	<div class="sub">
		{hosts ? `${hosts}, ` : ''}served {#if s.unit}by <span class="m">{s.unit.name}</span>{/if}
		on <a class="m ul" href={href({ screen: 'machine', name: s.machine })}>{s.machine}</a>
	</div>
	<div class="st">
		<Dot s={st.dot} />
		{#if st.reached}
			{st.reached.ok} of {st.reached.of} nodes reach it{st.failing.length
				? ` · ${timesOut(st.failing, snap.at)}`
				: ''}
		{:else}
			{s.unit ? `${s.unit.name} ${s.unit.state}` : 'nothing to probe'}{s.unit?.since
				? ` since ${when(s.unit.since, snap.at)}`
				: ''}
		{/if}
	</div>
</div>
{#if alerts.length}
	<div class="bands">
		{#each alerts as a (a.id)}<Band alert={a} nodes={snap.nodes} now={snap.at} />{/each}
	</div>
{/if}

{#if probed}
	<Label label="Probe map" value="tap a node" />
	<ProbeMap
		reach={st.reach}
		name={s.name}
		sub="{s.machine}{s.unit ? ` · ${s.unit.name}` : ''}"
		wide={layout.desk}
		bind:selected
	/>
	<Choices options={RANGES} on={range} pick={(r) => (range = r)} />
	<Label
		label="Latency from {selected}"
		value={sel?.ok === false
			? `timeout since ${when(sel.since!, snap.at)}`
			: `${latency(sel?.latency ?? null)} now`}
		tone={sel?.ok === false ? 'bad' : ''}
	/>
	{#if selSeries}
		<Chart
			lines={selSeries.data.kind === 'lines' ? selSeries.data.lines : []}
			from={selSeries.from}
			to={selSeries.to}
			now={snap.at}
			fmt={latency}
			area
			gaps={gaps(points(selSeries))}
			h={layout.desk ? 120 : 96}
			days={range === '7d'}
		/>
	{:else}
		<div class="ch" style:--h="{layout.desk ? 120 : 96}px"></div>
	{/if}

	<Label label="From each node" value="now · last {range}" />
	{#if layout.desk}
		<table class="tbl">
			<thead>
				<tr>
					<th></th><th>node</th><th>result</th><th class="n">now</th><th class="n">
						p50 {range}
					</th><th class="n">max {range}</th><th class="n">{range}</th>
				</tr>
			</thead>
			<tbody>
				{#each st.reach as r (r.node)}
					{@const x = perNode.get(r.node)?.value}
					{@const v = stats(x)}
					<tr
						class:sel={r.node === selected}
						data-node={r.node}
						onclick={() => (selected = r.node)}
					>
						<td><Dot s={r.ok === false ? 'bad' : r.ok ? 'ok' : 'idle'} /></td>
						<td class="m nm">{r.node}</td>
						<td class:bad={r.ok === false}>{result(r.ok)}</td>
						<td class="n m" class:bad={r.ok === false}>
							{r.ok === null ? '—' : latency(r.latency)}
						</td>
						<td class="n m">{latency(v.p50)}</td>
						<td class="n m">{latency(v.max)}</td>
						<td class="n" class:bad={r.ok === false}><Spark points={points(x)} w={96} h={16} /></td>
					</tr>
				{/each}
			</tbody>
		</table>
	{:else}
		{#each st.reach as r (r.node)}
			<button
				class="row"
				class:sel={r.node === selected}
				data-node={r.node}
				onclick={() => (selected = r.node)}
			>
				<Dot s={r.ok === false ? 'bad' : r.ok ? 'ok' : 'idle'} />
				<div>
					<div class="rh"><span class="nm m">{r.node}</span></div>
					<div class="sb" class:bad={r.ok === false} class:mute={r.ok !== false}>
						{result(r.ok)}{r.ok ? ` · ${latency(r.latency)}` : ''}
					</div>
				</div>
				<span class="rt" class:bad={r.ok === false}>
					<Spark points={points(perNode.get(r.node)?.value)} w={112} h={24} />
				</span>
			</button>
		{/each}
	{/if}

	<Label label="Latency, all nodes" value="median and range" />
	{#if band.value?.data.kind === 'band'}
		<Chart
			band={band.value.data.points}
			from={band.value.from}
			to={band.value.to}
			now={snap.at}
			fmt={latency}
			gaps={st.failing.flatMap((r) => gaps(points(perNode.get(r.node)?.value)))}
			h={96}
			days={range === '7d'}
		/>
	{:else}
		<div class="ch" style:--h="96px"></div>
	{/if}
{/if}

<Label label="Unit" />
<dl class="kv">
	{#if s.unit}
		<div>
			<dt>unit</dt>
			<dd class="m">{s.unit.name}</dd>
		</div>
		<div>
			<dt>state</dt>
			<dd class={unitDot === 'ok' ? 'ok' : unitDot === 'bad' ? 'bad' : ''}>{s.unit.state}</dd>
		</div>
	{:else}
		<div>
			<dt>unit</dt>
			<dd class="mute">none of its own</dd>
		</div>
	{/if}
	<div>
		<dt>on</dt>
		<dd><a class="m ul" href={href({ screen: 'machine', name: s.machine })}>{s.machine}</a></dd>
	</div>
	{#if s.unit?.since}<div>
			<dt>since</dt>
			<dd class="m">{when(s.unit.since, snap.at)}</dd>
		</div>{/if}
	{#each s.probes as p (p.target)}
		<div>
			<dt>{p.kind} probe</dt>
			<dd class="m ell">{p.target}</dd>
		</div>
	{/each}
</dl>

{#if !layout.desk}
	<Label label="Recent logs" value="all in the sheet" />
	<Lines lines={recent.lines} />
{/if}
