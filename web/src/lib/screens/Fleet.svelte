<!--
	The fleet. While something fires: how many things need you, and a severity band for each.
	Otherwise a calm title. Then every machine and service, and what resolved today; on desktop
	the machines and services are tables.
-->
<script lang="ts">
	import { untrack } from 'svelte';
	import { seriesUrl, Load } from '#lib/api.svelte.ts';
	import Band from '#lib/alert/Band.svelte';
	import AlertRow from '#lib/alert/AlertRow.svelte';
	import { date, hm, latency, load, pct, dur } from '#lib/derive/format.ts';
	import {
		byPresence,
		firingOf,
		headline,
		machineDot,
		machineTally,
		presence,
		resolvedOf,
		serviceStatus
	} from '#lib/derive/status.ts';
	import { href, router } from '#lib/router.svelte.ts';
	import MachineRow from '#lib/rows/MachineRow.svelte';
	import ServiceRow from '#lib/rows/ServiceRow.svelte';
	import Dot from '#lib/ui/Dot.svelte';
	import Gauge from '#lib/ui/Gauge.svelte';
	import Label from '#lib/ui/Label.svelte';
	import { layout } from '#lib/ui/layout.svelte.ts';
	import Spark from '#lib/ui/Spark.svelte';
	import { parseSeries } from '#lib/wire/parse.ts';
	import type { Series, Snapshot } from '#lib/wire/types.ts';

	let { snap }: { snap: Snapshot } = $props();

	const head = $derived(headline(snap));
	const firing = $derived(firingOf(snap));
	const resolved = $derived(resolvedOf(snap));
	const machines = $derived(byPresence(snap.machines));
	const statuses = $derived(
		new Map(snap.services.map((s) => [s.name, serviceStatus(s, snap.nodes)]))
	);
	const minute = () => Math.floor(snap.at / 60_000);

	/** the 6h sparklines of the desktop tables, one series each */
	const cpu = new Map<string, Load<Series>>();
	const lat = new Map<string, Load<Series>>();
	const registry = untrack(() => snap);
	for (const m of registry.machines)
		cpu.set(
			m.name,
			new Load(
				() => (layout.desk ? seriesUrl({ machine: m.name }, 'cpu', '6h') : null),
				parseSeries,
				minute
			)
		);
	for (const s of registry.services)
		if (s.probes.length)
			lat.set(
				s.name,
				new Load(
					() => (layout.desk ? seriesUrl({ service: s.name }, 'latency', '6h') : null),
					parseSeries,
					minute
				)
			);

	const linePoints = (s: Series | null | undefined) =>
		s?.data.kind === 'lines' ? (s.data.lines[0]?.points ?? []) : [];
	const medianPoints = (s: Series | null | undefined) =>
		s?.data.kind === 'band'
			? s.data.points.map((p) => [p[0], p[2]] as [number, number | null])
			: [];
</script>

<div class="tb">
	<div class="k">{date(snap.at)} · {hm(snap.at)}</div>
	{#if head.kind === 'firing'}
		<h1>
			<em>{head.count === 1 ? '1 thing' : `${head.count} things`}</em> need{head.count === 1
				? 's'
				: ''} you.
		</h1>
		<p class="sub">{head.say}</p>
	{:else}
		<h1>Fleet</h1>
		<div class="st"><Dot s={head.dot} />{head.say}</div>
	{/if}
</div>
{#if firing.length}
	<div class="bands">
		{#each firing as a (a.id)}<Band alert={a} nodes={snap.nodes} now={snap.at} />{/each}
	</div>
{/if}

<Label label="Machines" value={machineTally(snap.machines)} />
{#if layout.desk}
	<table class="tbl">
		<thead>
			<tr>
				<th></th><th>host</th><th>kind</th><th>cpu, 6h</th><th>mem</th><th>disk</th><th class="n">
					load
				</th><th class="n">units</th><th class="n">up</th>
			</tr>
		</thead>
		<tbody>
			{#each machines as m (m.name)}
				{@const go = () => router.go({ screen: 'machine', name: m.name })}
				{#if m.state.kind === 'up'}
					{@const st = m.state}
					<tr data-go data-machine={m.name} onclick={go}>
						<td><Dot s={machineDot(m)} /></td>
						<td class="m nm"><a href={href({ screen: 'machine', name: m.name })}>{m.name}</a></td>
						<td>{m.os} · {m.hardware?.arch ?? ''}</td>
						<td>
							<span class="gc">
								<Spark points={linePoints(cpu.get(m.name)?.value)} w={64} h={16} /><span class="m">
									{pct(st.vitals.cpu)}
								</span>
							</span>
						</td>
						<td class="m">
							<div class="gc"><Gauge v={st.vitals.mem} /><span>{pct(st.vitals.mem)}</span></div>
						</td>
						<td class="m">
							<div class="gc"><Gauge v={st.vitals.disk} /><span>{pct(st.vitals.disk)}</span></div>
						</td>
						<td class="n m">{load(st.vitals.load1)}</td>
						<td class="n m" class:bad={st.units.failed.length}>
							{st.units.failed.length ? `${st.units.failed.length} failed` : st.units.active}
						</td>
						<td class="n m">{dur(snap.at - st.booted_at)}</td>
					</tr>
				{:else}
					<tr class="muted" data-go data-machine={m.name} onclick={go}>
						<td><Dot s={machineDot(m)} /></td>
						<td class="m"><a href={href({ screen: 'machine', name: m.name })}>{m.name}</a></td>
						<td>{m.os}{m.always_on ? '' : ' desktop'}</td>
						<td colspan="6" class:bad={m.state.kind === 'down'}>{presence(m, snap.at)}</td>
					</tr>
				{/if}
			{/each}
		</tbody>
	</table>
{:else}
	{#each machines as m (m.name)}<MachineRow machine={m} now={snap.at} />{/each}
{/if}

<Label
	label="Services"
	value={layout.desk ? 'latency now, from each node' : `from ${snap.nodes.length} nodes`}
/>
{#if layout.desk}
	<table class="tbl">
		<thead>
			<tr>
				<th></th><th>service</th><th>unit</th>
				{#each snap.nodes as n (n.name)}<th class="n">{n.name}</th>{/each}
				<th class="n">6h</th>
			</tr>
		</thead>
		<tbody>
			{#each snap.services as s (s.name)}
				{@const st = statuses.get(s.name)!}
				<tr
					data-go
					data-service={s.name}
					onclick={() => router.go({ screen: 'service', name: s.name })}
				>
					<td><Dot s={st.dot} /></td>
					<td class="m nm"><a href={href({ screen: 'service', name: s.name })}>{s.name}</a></td>
					<td class:bad={s.unit?.state === 'failed'}>
						{s.unit ? `${s.unit.name.replace(/\.service$/, '')} · ${s.machine}` : s.machine}
					</td>
					{#each st.reach as r (r.node)}
						<td class="n m" class:bad={r.ok === false}>
							{r.ok === null ? '—' : latency(r.latency)}
						</td>
					{/each}
					<td class="n" class:warn={st.dot === 'warn'} class:bad={st.dot === 'bad'}>
						{#if st.reached}<Spark
								points={medianPoints(lat.get(s.name)?.value)}
								w={64}
								h={16}
							/>{:else}{s.unit?.state ?? ''}{/if}
					</td>
				</tr>
			{/each}
		</tbody>
	</table>
{:else}
	{#each snap.services as s (s.name)}<ServiceRow
			service={s}
			status={statuses.get(s.name)!}
		/>{/each}
{/if}

{#if resolved.length}
	<Label label="Resolved" value="{resolved.length} in 24h" />
	{#each resolved as a (a.id)}<AlertRow alert={a} now={snap.at} />{/each}
{/if}
