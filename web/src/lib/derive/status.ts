/**
 * What each thing in the fleet looks like, decided once from the snapshot: the dot a machine or
 * service shows, how many nodes reach a service, and how an alert asks for attention. Screens
 * read these and never decide status themselves.
 */
import type {
	Alert,
	Hostname,
	Machine,
	Ms,
	Node,
	Service,
	Severity,
	Snapshot
} from '#lib/wire/types.ts';
import { and, count, dur, hm, sentence, when } from './format.ts';

/** a status dot, as design/system.md lists them */
export type Dot = 'ok' | 'warn' | 'bad' | 'acc' | 'idle' | 'off' | 'asleep';

/**
 * The severity treatment an alert gets. The system draws two: critical (the `bad` wash, a
 * pulsing dot) and warning (the `warn` wash, a still dot). A `page` alert is meant to reach a
 * phone, so it takes the critical treatment; an `info` alert only shows up in the app, so it
 * takes the warning one.
 */
export type Treatment = 'crit' | 'warn';
export const TREATMENT: Record<Severity, Treatment> = { page: 'crit', info: 'warn' };

export const isFiring = (a: Alert) => a.state.kind === 'firing';

/** the dot of an alert: its treatment's color while firing, ok once resolved */
export const alertDot = (a: Alert): Dot =>
	a.state.kind === 'resolved' ? 'ok' : TREATMENT[a.severity] === 'crit' ? 'bad' : 'warn';

/** when it ended, or `now` while it fires */
export const endOf = (a: Alert, now: Ms) => (a.state.kind === 'resolved' ? a.state.at : now);

/** the host part of a probe target, a URL or `host:port` */
export const hostOf = (target: string) =>
	target.replace(/^[a-z]+:\/\//, '').replace(/[/:?].*$/, '');

/**
 * What an alert is about, in a few words: the title of its band, its hero and its sidebar
 * item. One line per rule the modules define; a rule this table does not know shows its name.
 */
const TITLES: Record<string, (a: Alert) => string | null> = {
	UnitFailed: (a) => `${a.labels.name ?? a.service ?? 'a unit'} failed`,
	MachineDown: (a) => a.machine && `${a.machine} is down`,
	ServiceUnreachable: (a) => a.service && `${a.service} unreachable`,
	CertExpiring: (a) => a.labels.target && `${hostOf(a.labels.target)} certificate expiring`,
	DiskAlmostFull: (a) =>
		a.machine && `${a.labels.mountpoint ?? 'disk'} almost full on ${a.machine}`,
	DiskFillingFast: (a) =>
		a.machine && `${a.labels.mountpoint ?? 'disk'} filling fast on ${a.machine}`,
	NodeUnhealthy: (a) => a.machine && `${a.labels.job ?? 'a store'} unhealthy on ${a.machine}`,
	LogsBacklog: (a) => a.machine && `logs backing up on ${a.machine}`
};

export const alertTitle = (a: Alert) => TITLES[a.rule]?.(a) || a.rule;

/** the title, and the machine it happens on when the title does not say */
export function alertSay(a: Alert): string {
	const title = alertTitle(a);
	return a.machine && !title.includes(a.machine) ? `${title} on ${a.machine}` : title;
}

/** the summary the rule writes, as a sentence */
export const alertProse = (a: Alert) => sentence(a.summary);

/** `for 43m · since 14:59`, or `11:07 to 13:20 · lasted 2h 13m` */
export function alertSpan(a: Alert, now: Ms): string {
	if (a.state.kind === 'resolved')
		return `${when(a.starts_at, now)} to ${hm(a.state.at)} · lasted ${dur(a.state.at - a.starts_at)}`;
	return `for ${dur(now - a.starts_at)} · since ${when(a.starts_at, now)}`;
}

export function machineDot(m: Machine): Dot {
	switch (m.state.kind) {
		case 'up':
			return m.state.units.failed.length ? 'bad' : 'ok';
		case 'down':
			return 'bad';
		case 'resting':
			return 'asleep';
	}
}

/** one node's view of a service: every probe from it ok, and the slowest of them */
export type Reach = {
	node: Hostname;
	/** null when the node ran none of its probes */
	ok: boolean | null;
	/** the slowest probe's latency, null when one failed or none ran */
	latency: number | null;
	/** when the first failing probe from this node started failing */
	since: Ms | null;
};

export function reach(s: Service, nodes: Node[]): Reach[] {
	return nodes.map(({ name }) => {
		const views = s.probes.flatMap((p) => p.views.filter((v) => v.node === name));
		if (!views.length) return { node: name, ok: null, latency: null, since: null };
		const failed = views.filter((v) => !v.ok);
		return {
			node: name,
			ok: !failed.length,
			latency: failed.length ? null : Math.max(...views.map((v) => v.latency_ms)),
			since: failed.length ? Math.min(...failed.map((v) => v.since)) : null
		};
	});
}

export const median = (values: number[]) => {
	const v = [...values].sort((a, b) => a - b);
	return v.length ? v[Math.floor(v.length / 2)] : null;
};

/** a service at a glance, for its row, its sidebar item and its title block */
export type ServiceStatus = {
	dot: Dot;
	reach: Reach[];
	/** nodes that reach it, of those that probe it; null for a service with no probes */
	reached: { ok: number; of: number } | null;
	/** the median of the latencies from the nodes that reach it */
	median: number | null;
	/** the nodes that cannot reach it */
	failing: Reach[];
};

export function serviceStatus(s: Service, nodes: Node[]): ServiceStatus {
	const r = reach(s, nodes);
	const probed = r.filter((x) => x.ok !== null);
	const failing = probed.filter((x) => x.ok === false);
	const unitFailed = s.unit?.state === 'failed';
	const dot: Dot =
		unitFailed || (probed.length && failing.length === probed.length)
			? 'bad'
			: failing.length
				? 'warn'
				: s.unit?.state === 'inactive' || s.unit?.state === 'unknown'
					? 'idle'
					: 'ok';
	return {
		dot,
		reach: r,
		reached: s.probes.length ? { ok: probed.length - failing.length, of: probed.length } : null,
		median: median(probed.flatMap((x) => (x.latency == null ? [] : [x.latency]))),
		failing
	};
}

/** `lab times out since 15:21` */
export const timesOut = (failing: Reach[], now: Ms) =>
	failing.length
		? `${and(failing.map((x) => x.node))} time${failing.length === 1 ? 's' : ''} out since ${when(Math.min(...failing.map((x) => x.since!)), now)}`
		: '';

/** what the fleet screen leads with */
export type Headline =
	{ kind: 'firing'; count: number; say: string } | { kind: 'clear'; dot: Dot; say: string };

export function headline(snap: Snapshot): Headline {
	const firing = snap.alerts.filter(isFiring);
	const resting = snap.machines.filter((m) => m.state.kind === 'resting').map((m) => m.name);
	const partial = snap.services.flatMap((s) => {
		const st = serviceStatus(s, snap.nodes);
		return st.failing.length && st.failing.length < (st.reached?.of ?? 0)
			? [`${and(st.failing.map((x) => x.node))} can’t reach ${s.name}`]
			: [];
	});
	const rest = resting.length ? `${and(resting)} ${resting.length === 1 ? 'rests' : 'rest'}.` : '';
	if (firing.length) {
		// identifiers lead the sentence as they are written, lower case and all
		const say = `${and([...firing.map(alertSay), ...partial])}. Everything else is up${rest ? `; ${rest}` : '.'}`;
		return { kind: 'firing', count: firing.length, say };
	}
	const nodesUp = snap.nodes.filter(
		(n) => snap.machines.find((m) => m.name === n.name)?.state.kind === 'up'
	).length;
	const say = [
		partial.length ? `Nothing firing · ${and(partial)}` : 'All clear',
		`${nodesUp} of ${count(snap.nodes.length, 'node')} up`,
		resting.length ? `${count(resting.length, 'desktop')} resting` : ''
	];
	return {
		kind: 'clear',
		dot: partial.length ? 'warn' : 'ok',
		say: say.filter(Boolean).join(' · ')
	};
}

/** `4 up · 2 resting` */
export function machineTally(machines: Machine[]): string {
	const by = (k: Machine['state']['kind']) => machines.filter((m) => m.state.kind === k).length;
	return [
		`${by('up')} up`,
		by('down') ? `${by('down')} down` : '',
		by('resting') ? `${by('resting')} resting` : ''
	]
		.filter(Boolean)
		.join(' · ');
}

/** machines that answer first, then the ones that rest, each in registry order */
export const byPresence = (machines: Machine[]) => [
	...machines.filter((m) => m.state.kind !== 'resting'),
	...machines.filter((m) => m.state.kind === 'resting')
];

/** `resting since 13:05` */
export function presence(m: Machine, now: Ms): string | null {
	if (m.state.kind === 'up') return null;
	return `${m.state.kind} since ${when(m.state.since, now)}`;
}

/** firing alerts newest first, then resolved ones newest first, as the snapshot orders them */
export const firingOf = (snap: Snapshot) => snap.alerts.filter(isFiring);
export const resolvedOf = (snap: Snapshot) => snap.alerts.filter((a) => !isFiring(a));

/** the firing alerts about a machine, or about a service */
export const alertsOn = (snap: Snapshot, on: { machine?: Hostname; service?: string }) =>
	firingOf(snap).filter((a) => (on.service ? a.service === on.service : a.machine === on.machine));
