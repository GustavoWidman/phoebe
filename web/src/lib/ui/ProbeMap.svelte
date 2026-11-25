<!--
	The probing nodes and the service, one path from each node to it, with the latency now on
	each path. Phone lays the nodes in a row above the service, desktop in a column left of it.
	Tapping a node selects it.
-->
<script lang="ts">
	import { latency } from '#lib/derive/format.ts';
	import type { Reach } from '#lib/derive/status.ts';

	let {
		reach,
		name,
		sub,
		wide,
		selected = $bindable()
	}: { reach: Reach[]; name: string; sub: string; wide: boolean; selected: string } = $props();

	const W = $derived(wide ? 720 : 358);
	const H = $derived(wide ? 248 : 236);
	const step = $derived(wide ? 54 : (W - 72) / Math.max(1, reach.length - 1));
	const pos = $derived(reach.map((_, i) => (wide ? [180, 44 + i * step] : [36 + i * step, 56])));
	const svc = $derived(wide ? [500, 125] : [W / 2, H - 64]);
	const f = $derived(wide ? 0.4 : 0.42);
</script>

<svg class="map" viewBox="0 0 {W} {H}" role="group" aria-label="probe map">
	{#each reach as r, i (r.node)}
		{@const [px, py] = pos[i]}
		{@const lx = px + (svc[0] - px) * f}
		{@const ly = py + (svc[1] - py) * f}
		{@const text = r.ok === null ? '—' : latency(r.latency)}
		{@const lw = text.length * 6.6 + 8}
		<g
			class:x={r.ok === false}
			class:sel={r.node === selected}
			data-node={r.node}
			role="button"
			tabindex="0"
			aria-pressed={r.node === selected}
			aria-label="{r.node}, {text}"
			onclick={() => (selected = r.node)}
			onkeydown={(e) => (e.key === 'Enter' || e.key === ' ') && (selected = r.node)}
		>
			<path class="p" d="M{px},{py}L{svc[0]},{svc[1]}" />
			<circle cx={px} cy={py} r="24" class="hit" />
			<circle cx={px} cy={py} r={r.node === selected ? 6 : 5} class="nd" />
			<text
				class="n"
				x={wide ? px - 16 : px}
				y={wide ? py + 4 : py - 16}
				text-anchor={wide ? 'end' : 'middle'}
			>
				{r.node}
			</text>
			<rect class="lb" x={lx - lw / 2} y={ly - 8} width={lw} height="16" />
			<text class="v" x={lx} y={ly + 4} text-anchor="middle">{text}</text>
		</g>
	{/each}
	<circle cx={svc[0]} cy={svc[1]} r="8" class="svc" />
	<text
		class="sn"
		x={wide ? svc[0] + 20 : svc[0]}
		y={wide ? svc[1] - 2 : svc[1] + 28}
		text-anchor={wide ? 'start' : 'middle'}
	>
		{name}
	</text>
	<text
		class="h"
		x={wide ? svc[0] + 20 : svc[0]}
		y={wide ? svc[1] + 14 : svc[1] + 44}
		text-anchor={wide ? 'start' : 'middle'}
	>
		{sub}
	</text>
</svg>
