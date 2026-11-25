/**
 * An alert's timeline as the screen tells it: each node's pending, firing and resolved, folded
 * into one event per run of the same kind, and every notification sent.
 */
import type { Hostname, Ms, TimelineEntry } from '#lib/wire/types.ts';
import { and } from './format.ts';
import type { Dot } from './status.ts';

export type Event = { at: Ms; dot: Dot; text: string };

const DOT: Record<TimelineEntry['kind'], Dot> = {
	pending: 'off',
	firing: 'bad',
	resolved: 'ok',
	notified: 'off'
};

/** `oracle-1, then the 3 other nodes`, or `oracle-1, then lab` */
function spread(nodes: Hostname[], total: number): string {
	const [first, ...rest] = nodes;
	if (!rest.length) return first;
	if (nodes.length === total)
		return `${first}, then the ${rest.length} other node${rest.length === 1 ? '' : 's'}`;
	return `${first}, then ${and(rest)}`;
}

export function events(entries: TimelineEntry[], total: number): Event[] {
	const out: Event[] = [];
	type Run = { kind: 'pending' | 'firing' | 'resolved'; at: Ms; nodes: Hostname[] };
	let run = null as Run | null;
	const close = () => {
		if (!run) return;
		const text =
			run.kind === 'pending'
				? `Pending on ${spread(run.nodes, total)}`
				: run.kind === 'firing'
					? `Fires on ${spread(run.nodes, total)}`
					: run.nodes.length === total && total > 1
						? `Resolved on all ${total} nodes`
						: `Resolved on ${and(run.nodes)}`;
		out.push({ at: run.at, dot: DOT[run.kind], text });
		run = null;
	};
	for (const e of entries) {
		if (e.kind === 'notified') {
			close();
			out.push({
				at: e.at,
				dot: DOT.notified,
				text: e.resolved ? `Resolve notice sent by ${e.by}` : `Notification sent by ${e.by}`
			});
		} else if (run?.kind === e.kind) run.nodes.push(e.node);
		else {
			close();
			run = { kind: e.kind, at: e.at, nodes: [e.node] };
		}
	}
	close();
	return out;
}

/** when each node first fired it */
export function firedAt(entries: TimelineEntry[]): Map<Hostname, Ms> {
	const at = new Map<Hostname, Ms>();
	for (const e of entries) if (e.kind === 'firing' && !at.has(e.node)) at.set(e.node, e.at);
	return at;
}
