<!-- a 1.5 line in the text's color, scaled from zero to its highest point -->
<script lang="ts">
	import { path } from './plot.ts';

	let {
		points,
		w = 120,
		h = 22
	}: { points: [number, number | null][]; w?: number; h?: number } = $props();

	const d = $derived.by(() => {
		if (points.length < 2) return '';
		const hi = Math.max(0, ...points.map((p) => p[1] ?? 0)) * 1.2 || 1;
		return path(points, { from: points[0][0], to: points.at(-1)![0], lo: 0, hi, w, h });
	});
</script>

<svg
	class="spk"
	width={w}
	height={h}
	viewBox="0 0 {w} {h}"
	preserveAspectRatio="none"
	aria-hidden="true"
>
	<path {d} />
</svg>
