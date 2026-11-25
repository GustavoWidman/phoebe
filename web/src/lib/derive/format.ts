/**
 * How the app writes times, durations and quantities: one function per kind of value, so a
 * number reads the same on every screen.
 */
import type { Ms } from '#lib/wire/types.ts';

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

const clock = new Intl.DateTimeFormat('en-GB', {
	hour: '2-digit',
	minute: '2-digit',
	hourCycle: 'h23'
});
const clockS = new Intl.DateTimeFormat('en-GB', {
	hour: '2-digit',
	minute: '2-digit',
	second: '2-digit',
	hourCycle: 'h23'
});
const day = new Intl.DateTimeFormat('en-GB', { weekday: 'short', day: 'numeric', month: 'short' });
const dayKey = new Intl.DateTimeFormat('en-CA', {
	year: 'numeric',
	month: '2-digit',
	day: '2-digit'
});

/** 15:42 */
export const hm = (t: Ms) => clock.format(t);
/** 15:42:07 */
export const hms = (t: Ms) => clockS.format(t);
/** Wed 25 Nov */
export const date = (t: Ms) => day.format(t).replace(',', '');

/** `hm`, with the day before it when `t` is not on the day of `now` */
export function when(t: Ms, now: Ms): string {
	if (dayKey.format(t) === dayKey.format(now)) return hm(t);
	if (dayKey.format(t) === dayKey.format(now - DAY)) return `yesterday ${hm(t)}`;
	return `${date(t)} ${hm(t)}`;
}

/** 43m, 2h 13m, 23d 4h */
export function dur(ms: number): string {
	const m = Math.max(0, Math.round(ms / MINUTE));
	if (m < 60) return `${m}m`;
	const h = Math.floor(m / 60);
	if (h < 24) return `${h}h ${m % 60}m`;
	return `${Math.floor(h / 24)}d ${h % 24}h`;
}

/** a fraction as a whole percent */
export const pct = (v: number | null) => (v == null ? '—' : `${Math.round(v * 100)}%`);

/** a latency; a probe that failed reads as its timeout */
export const latency = (v: number | null) =>
	v == null ? 'timeout' : v < 10 ? `${v.toFixed(1)} ms` : `${Math.round(v)} ms`;

/** bytes per second */
export function rate(v: number | null): string {
	if (v == null) return '—';
	if (v >= 1e9) return `${(v / 1e9).toFixed(1)} GB/s`;
	if (v >= 1e6) return `${(v / 1e6).toFixed(1)} MB/s`;
	if (v >= 1e3) return `${Math.round(v / 1e3)} kB/s`;
	return `${Math.round(v)} B/s`;
}

/** a size in GiB, the way memory and disks are sold */
export function gib(bytes: number): string {
	const g = bytes / 2 ** 30;
	return g >= 1000 ? `${(g / 1024).toFixed(1)} TiB` : `${Math.round(g)} GiB`;
}

export const load = (v: number | null) => (v == null ? '—' : v.toFixed(2));

/** `n thing`, or `n things` */
export const count = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

/** a, b and c */
export function and(items: string[]): string {
	if (items.length < 2) return items.join('');
	return `${items.slice(0, -1).join(', ')} and ${items.at(-1)}`;
}

/** a sentence, capitalized unless it starts with an identifier, which keeps its spelling */
export const sentence = (s: string) =>
	`${/^[a-z]+\s/.test(s) ? s.charAt(0).toUpperCase() + s.slice(1) : s}`.replace(/\.?$/, '.');
