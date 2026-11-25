/**
 * The fleet as `/api/stream` last described it: one snapshot, replaced whole by the next, and
 * the lookups every screen makes into it. The stream opens once, when the app starts, and every
 * screen reads this one store.
 */
import { parseStreamEvent } from '#lib/wire/parse.ts';
import type {
	Alert,
	AlertId,
	Hostname,
	Machine,
	Service,
	Snapshot,
	StreamEvent
} from '#lib/wire/types.ts';
import { stream, type Link } from './sse.ts';

export class Fleet {
	snapshot = $state.raw<Snapshot | null>(null);
	link = $state<Link>('connecting');

	machines = $derived(new Map<Hostname, Machine>(this.snapshot?.machines.map((m) => [m.name, m])));
	services = $derived(new Map<string, Service>(this.snapshot?.services.map((s) => [s.name, s])));
	alerts = $derived(new Map<AlertId, Alert>(this.snapshot?.alerts.map((a) => [a.id, a])));

	/** opens the stream; returns what closes it */
	connect(url = '/api/stream') {
		return stream<StreamEvent>(url, {
			parse: parseStreamEvent,
			events: (events) => {
				for (const e of events) if (e.kind === 'snapshot') this.snapshot = e;
			},
			link: (state) => (this.link = state)
		});
	}
}

export const fleet = new Fleet();
