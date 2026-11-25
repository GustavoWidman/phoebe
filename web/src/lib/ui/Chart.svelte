<!--
	A plot 600 by 100 units stretched to its column: three hairline grid lines, the y max top
	right, four time ticks under it. Lines in acc (a second one in mute), an optional area under
	the first, a min-to-max band with its median, and a dashed mark where an alert fired. Where
	the first line or the band has no value, a quiet wash says nothing was measured; where a
	probe failed, the bad wash says it timed out. A pointer over it shows a crosshair and the
	values at that moment.
-->
<script lang="ts">
	import { hm } from '#lib/derive/format.ts';
	import type { Ms } from '#lib/wire/types.ts';
	import { area, between, gaps, path, xOf, type Frame } from './plot.ts';

	type Point = [Ms, number | null];
	type Props = {
		lines?: { name: string; points: Point[] }[];
		band?: [Ms, number | null, number | null, number | null][];
		from: Ms;
		to: Ms;
		/** the moment the fleet is at; a tick there reads `now` */
		now: Ms;
		min?: number;
		max?: number;
		area?: boolean;
		mark?: Ms | null;
		/** the spans a probe failed, which a timeout's wash shows */
		failing?: [Ms, Ms][];
		fmt: (v: number | null) => string;
		h?: number;
		/** tick labels: times of day, or days for a week */
		days?: boolean;
	};
	let {
		lines = [],
		band,
		from,
		to,
		now,
		min = 0,
		max,
		area: filled = false,
		mark = null,
		failing = [],
		fmt,
		h = 96,
		days = false
	}: Props = $props();

	const W = 600;
	const H = 100;
	const values = $derived([
		...lines.flatMap((l) => l.points.map((p) => p[1])),
		...(band ?? []).map((p) => p[3])
	]);
	const hi = $derived(max ?? (Math.max(...values.map((v) => v ?? 0)) * 1.15 || 1));
	const frame = $derived<Frame>({ from, to, lo: min, hi, w: W, h: H });
	const first = $derived(lines[0]?.points ?? band?.map((p) => [p[0], p[2]] as Point) ?? []);
	const empty = $derived(gaps(first));
	const dayFmt = new Intl.DateTimeFormat('en-GB', { day: 'numeric', month: 'short' });
	const ticks = $derived(
		[0, 1 / 3, 2 / 3, 1].map((f) => {
			const t = from + (to - from) * f;
			return f === 1 && Math.abs(to - now) < 60_000 ? 'now' : days ? dayFmt.format(t) : hm(t);
		})
	);

	let at = $state<number | null>(null);
	let width = $state(0);
	const point = $derived.by(() => {
		if (at == null || !first.length) return null;
		const t = from + (to - from) * at;
		let i = 0;
		for (let j = 1; j < first.length; j++)
			if (Math.abs(first[j][0] - t) < Math.abs(first[i][0] - t)) i = j;
		const time = first[i][0];
		const measured = first[i][1] != null || failing.some(([a, b]) => a <= time && time <= b);
		const said = !measured
			? 'no data'
			: band
				? fmt(band[i]?.[2] ?? null)
				: lines
						.map((l) => {
							const v = l.points.find((p) => p[0] === time)?.[1] ?? null;
							return lines.length > 1 ? `${l.name} ${fmt(v)}` : fmt(v);
						})
						.join(' · ');
		return { x: xOf(time, frame) / W, tip: `${hm(time)} · ${said}` };
	});

	function move(e: PointerEvent) {
		const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
		at = Math.max(0, Math.min(1, (e.clientX - r.left) / r.width));
	}
</script>

<div
	class="ch"
	class:on={point != null}
	style:--h="{h}px"
	bind:clientWidth={width}
	onpointermove={move}
	onpointerdown={move}
	onpointerleave={() => (at = null)}
	onpointercancel={() => (at = null)}
	role="img"
	aria-label="chart"
>
	<svg viewBox="0 0 {W} {H}" preserveAspectRatio="none" aria-hidden="true">
		{#each [0, 0.5, 1] as f (f)}<path class="gr" d="M0,{H * f}H{W}" />{/each}
		{#each empty as [a, b], i (i)}
			<rect class="none" x={xOf(a, frame)} y="0" width={xOf(b, frame) - xOf(a, frame)} height={H} />
		{/each}
		{#each failing as [a, b], i (i)}
			<rect class="gap" x={xOf(a, frame)} y="0" width={xOf(b, frame) - xOf(a, frame)} height={H} />
		{/each}
		{#if band}
			<path class="ar" d={between(band, frame)} />
			<path
				class="ln"
				d={path(
					band.map((p) => [p[0], p[2]]),
					frame
				)}
			/>
		{/if}
		{#if filled && lines[0]}<path class="ar" d={area(lines[0].points, frame)} />{/if}
		{#each lines as l, i (l.name)}
			<path class="ln" class:mute={i > 0} d={path(l.points, frame)} />
		{/each}
		{#if mark != null && mark >= from && mark <= to}
			<path class="mk" d="M{xOf(mark, frame)},0V{H}" />
		{/if}
	</svg>
	<div class="ch-y m">{fmt(hi)}</div>
	{#if point}
		<div class="ch-x" style:left="{point.x * 100}%"></div>
		<div class="ch-tip" style:left="{Math.max(60, Math.min(width - 60, point.x * width))}px">
			{point.tip}
		</div>
	{/if}
</div>
<div class="ch-t">
	{#each ticks as t, i (i)}<span>{t}</span>{/each}
</div>
