/** Points to SVG paths, placed by their time between `from` and `to`. A null breaks the line. */
import type { Ms } from '#lib/wire/types.ts';

export type Frame = { from: Ms; to: Ms; lo: number; hi: number; w: number; h: number };

export const xOf = (t: Ms, f: Frame) => ((t - f.from) / (f.to - f.from || 1)) * f.w;
export const yOf = (v: number, f: Frame) => f.h - ((v - f.lo) / (f.hi - f.lo || 1)) * f.h;
const n = (v: number) => v.toFixed(1);

export function path(points: [Ms, number | null][], f: Frame): string {
	let d = '';
	let pen = false;
	for (const [t, v] of points) {
		if (v == null) {
			pen = false;
			continue;
		}
		d += `${pen ? 'L' : 'M'}${n(xOf(t, f))},${n(yOf(v, f))}`;
		pen = true;
	}
	return d;
}

/** the area under each unbroken run of the line */
export function area(points: [Ms, number | null][], f: Frame): string {
	let d = '';
	let run: [Ms, number][] = [];
	const close = () => {
		if (run.length > 1)
			d += path(run, f) + `L${n(xOf(run.at(-1)![0], f))},${f.h}L${n(xOf(run[0][0], f))},${f.h}Z`;
		run = [];
	};
	for (const [t, v] of points) {
		if (v == null) close();
		else run.push([t, v]);
	}
	close();
	return d;
}

/** the area between a low and a high edge, over the points that have both */
export function between(points: [Ms, number | null, number | null, number | null][], f: Frame) {
	const have = points.filter((p) => p[1] != null && p[3] != null) as [Ms, number, number, number][];
	if (have.length < 2) return '';
	const top = have.map(([t, , , hi]) => `${n(xOf(t, f))},${n(yOf(hi, f))}`);
	const bottom = have.toReversed().map(([t, lo]) => `${n(xOf(t, f))},${n(yOf(lo, f))}`);
	return `M${top.join('L')}L${bottom.join('L')}Z`;
}

/** the spans, `[from, to]`, where the line has no value */
export function gaps(points: [Ms, number | null][]): [Ms, Ms][] {
	const out: [Ms, Ms][] = [];
	let start: Ms | null = null;
	points.forEach(([t, v], i) => {
		if (v == null && start == null) start = i ? points[i - 1][0] : t;
		if (v != null && start != null) {
			out.push([start, t]);
			start = null;
		}
	});
	if (start != null) out.push([start, points.at(-1)![0]]);
	return out;
}
