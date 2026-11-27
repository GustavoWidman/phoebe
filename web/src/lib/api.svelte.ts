/**
 * The routes that answer on demand: series, log pages and alert details. Each answer is checked
 * against the contract on arrival, and a screen holds one in a `Load` that asks again whenever
 * what it asks for changes. Every request carries the session; one it no longer opens sends the
 * app to sign in.
 */
import { refused, signed } from '#lib/auth/access.ts';
import { ParseError, type Parser } from '#lib/wire/parse.ts';
import type { Metric } from '#lib/wire/types.ts';

/** a service as the api names it: `<machine>/<name>` */
export const servicePath = (s: { machine: string; name: string }) => `${s.machine}/${s.name}`;

export async function get<T>(url: string, parse: Parser<T>, signal?: AbortSignal): Promise<T> {
	const res = await signed(url, { signal });
	if (res.status === 401) void refused();
	if (!res.ok) throw new Error(`${url}: ${res.status}`);
	return parse(await res.json(), url);
}

export type Range = '1h' | '6h' | '24h' | '7d';
export const RANGES: Range[] = ['1h', '6h', '24h', '7d'];

const query = (params: Record<string, string | number | null | undefined>) => {
	const q = new URLSearchParams();
	for (const [k, v] of Object.entries(params)) if (v != null && v !== '') q.set(k, String(v));
	return q.toString();
};

export const seriesUrl = (
	of: { machine: string } | { service: { machine: string; name: string } },
	metric: Metric,
	range: Range,
	node?: string
) =>
	`/api/series?${query({
		of: 'machine' in of ? `machine:${of.machine}` : `service:${servicePath(of.service)}`,
		metric,
		range,
		node
	})}`;

export type LogFilter = {
	machine?: string;
	unit?: string;
	service?: { machine: string; name: string };
	q?: string;
};
const logQuery = (f: LogFilter) => ({ ...f, service: f.service && servicePath(f.service) });
export const logsUrl = (f: LogFilter, limit: number) =>
	`/api/logs?${query({ ...logQuery(f), limit })}`;
export const tailUrl = (f: LogFilter) => `/api/logs/tail?${query(logQuery(f))}`;
export const alertUrl = (id: string) => `/api/alerts/${encodeURIComponent(id)}`;

/**
 * One answer, asked for again whenever `url` or `again` changes. The last answer stays while
 * the next is on its way, so a chart keeps its line while its range changes.
 */
export class Load<T> {
	value = $state.raw<T | null>(null);
	failed = $state(false);

	constructor(url: () => string | null, parse: Parser<T>, again: () => unknown = () => 0) {
		$effect(() => {
			const u = url();
			again();
			if (!u) {
				this.value = null;
				return;
			}
			const abort = new AbortController();
			get(u, parse, abort.signal).then(
				(v) => {
					this.value = v;
					this.failed = false;
				},
				(e) => {
					if (abort.signal.aborted) return;
					if (e instanceof ParseError) console.warn(`phoebe: ${e.message}`);
					this.failed = true;
				}
			);
			return () => abort.abort();
		});
	}
}
