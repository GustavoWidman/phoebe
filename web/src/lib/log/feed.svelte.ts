/**
 * Log lines for one filter: the newest page from `/api/logs`, then, while the feed is live,
 * every line `/api/logs/tail` brings, oldest first. A line that arrived through the tail is
 * fresh, and fades in once.
 */
import { get, logsUrl, tailUrl, type LogFilter } from '#lib/api.svelte.ts';
import { stream, type Link } from '#lib/stream/sse.ts';
import { parseLogEvent, parseLogPage } from '#lib/wire/parse.ts';
import type { LogEvent, LogLine } from '#lib/wire/types.ts';

export type Shown = LogLine & { key: number; fresh: boolean };

let next = 0;
const shown = (l: LogLine, fresh: boolean): Shown => ({ ...l, key: next++, fresh });

export class LogFeed {
	lines = $state.raw<Shown[]>([]);
	link = $state<Link>('connecting');

	/**
	 * `filter` is what to show, or null for nothing; `live` whether to follow the tail; `limit`
	 * how many lines to keep.
	 */
	constructor(filter: () => LogFilter | null, live: () => boolean, limit: number) {
		$effect(() => {
			const f = filter();
			const follow = live();
			if (!f) {
				this.lines = [];
				return;
			}
			const abort = new AbortController();
			let stop: (() => void) | null = null;
			get(logsUrl(f, limit), parseLogPage, abort.signal).then(
				(page) => {
					this.lines = page.lines.toReversed().map((l) => shown(l, false));
					if (!follow) return;
					stop = stream<LogEvent>(tailUrl(f), {
						parse: parseLogEvent,
						events: (events) => {
							const got = events.flatMap((e) => (e.kind === 'lines' ? e.lines : []));
							if (got.length)
								this.lines = [...this.lines, ...got.map((l) => shown(l, true))].slice(-limit);
						},
						link: (state) => (this.link = state)
					});
				},
				() => {
					if (!abort.signal.aborted) this.link = 'retrying';
				}
			);
			return () => {
				abort.abort();
				stop?.();
			};
		});
	}
}
