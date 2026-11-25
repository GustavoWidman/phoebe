/**
 * Every answer the server sends, checked against the types it declares. Everything past here
 * trusts its types. A field the app does not know is dropped, so a newer server may add fields.
 */
import type {
	Alert,
	AlertDetail,
	AlertId,
	FailedUnit,
	Hardware,
	Hello,
	Line,
	LogEvent,
	LogLine,
	LogPage,
	Machine,
	MachineState,
	Ms,
	Node,
	Probe,
	ProbeView,
	PushView,
	Series,
	SeriesData,
	Service,
	ServiceName,
	Hostname,
	SignedIn,
	Snapshot,
	StreamEvent,
	Subject,
	TimelineEntry,
	UnitStatus,
	Viewer
} from './types.ts';

export class ParseError extends Error {}

export type Parser<T> = (value: unknown, at: string) => T;

const fail = (at: string, want: string): never => {
	throw new ParseError(`${at}: not ${want}`);
};

const str: Parser<string> = (v, at) => (typeof v === 'string' ? v : fail(at, 'a string'));
const num: Parser<number> = (v, at) =>
	typeof v === 'number' && Number.isFinite(v) ? v : fail(at, 'a number');
const bool: Parser<boolean> = (v, at) => (typeof v === 'boolean' ? v : fail(at, 'true or false'));
const ms = num as Parser<Ms>;
const host = str as Parser<Hostname>;
const service = str as Parser<ServiceName>;
const alertId = str as Parser<AlertId>;

const nullable =
	<T>(p: Parser<T>): Parser<T | null> =>
	(v, at) =>
		v === null ? null : p(v, at);

const list =
	<T>(p: Parser<T>): Parser<T[]> =>
	(v, at) =>
		Array.isArray(v) ? v.map((x, i) => p(x, `${at}[${i}]`)) : fail(at, 'a list');

const oneOf =
	<T extends string>(...values: T[]): Parser<T> =>
	(v, at) =>
		values.includes(v as T) ? (v as T) : fail(at, values.join(' or '));

const record = (v: unknown, at: string) =>
	typeof v === 'object' && v !== null && !Array.isArray(v)
		? (v as Record<string, unknown>)
		: fail(at, 'an object');

function obj<T>(shape: { [K in keyof T]-?: Parser<T[K]> }): Parser<T> {
	return (v, at) => {
		const r = record(v, at);
		const out = {} as T;
		for (const k in shape) out[k] = shape[k](r[k], `${at}.${k}`);
		return out;
	};
}

type Cases<T extends { kind: string }> = {
	[K in T['kind']]: Parser<Omit<Extract<T, { kind: K }>, 'kind'>>;
};

function tagged<T extends { kind: string }>(cases: Cases<T>): Parser<T> {
	return (v, at) => {
		const kind = record(v, at).kind as T['kind'];
		if (typeof kind !== 'string' || !Object.hasOwn(cases, kind))
			fail(`${at}.kind`, Object.keys(cases).join(' or '));
		return { kind, ...cases[kind](v, at) } as unknown as T;
	};
}

const none: Parser<Record<never, never>> = (v, at) => (record(v, at), {});

const strings: Parser<{ [key in string]: string }> = (v, at) => {
	const r = record(v, at);
	const out: Record<string, string> = {};
	for (const k in r) out[k] = str(r[k], `${at}.${k}`);
	return out;
};

/** a tuple whose every slot has its own parser */
const tuple =
	<T extends unknown[]>(...ps: { [K in keyof T]: Parser<T[K]> }): Parser<T> =>
	(v, at) =>
		Array.isArray(v) && v.length === ps.length
			? (ps.map((p, i) => p(v[i], `${at}[${i}]`)) as T)
			: fail(at, `a list of ${ps.length}`);

const node = obj<Node>({
	name: host,
	prometheus: bool,
	loki: bool,
	alertmanager: bool,
	blackbox: bool
});

const machineState = tagged<MachineState>({
	up: obj({
		since: ms,
		booted_at: ms,
		vitals: obj({ cpu: num, mem: num, disk: num, load1: num }),
		units: obj({ active: num, failed: list(obj<FailedUnit>({ name: str, since: ms })) })
	}),
	down: obj({ since: ms }),
	resting: obj({ since: ms })
});

const machine = obj<Machine>({
	name: host,
	os: oneOf('nixos', 'darwin'),
	always_on: bool,
	hardware: nullable(obj<Hardware>({ arch: str, cores: num, mem_bytes: num, disk_bytes: num })),
	state: machineState
});

const probe = obj<Probe>({
	kind: oneOf('http', 'tcp', 'smtp', 'imap', 'dns'),
	target: str,
	tls: bool,
	cert_expires_at: nullable(ms),
	views: list(obj<ProbeView>({ node: host, ok: bool, latency_ms: num, since: ms }))
});

const svc = obj<Service>({
	name: service,
	machine: host,
	unit: nullable(
		obj<UnitStatus>({
			name: str,
			state: oneOf('active', 'activating', 'inactive', 'failed', 'unknown'),
			since: nullable(ms)
		})
	),
	probes: list(probe)
});

const alert = obj<Alert>({
	id: alertId,
	rule: str,
	severity: oneOf('page', 'info'),
	state: tagged<Alert['state']>({ firing: none, resolved: obj({ at: ms }) }),
	starts_at: ms,
	machine: nullable(host),
	service: nullable(service),
	summary: str,
	seen_by: list(host),
	labels: strings
});

const snapshotShape = obj<Snapshot>({
	schema: num,
	at: ms,
	served_by: host,
	nodes: list(node),
	machines: list(machine),
	services: list(svc),
	alerts: list(alert)
});

const streamEvent = tagged<StreamEvent>({
	snapshot: snapshotShape,
	expired: none
} as Cases<StreamEvent>);

const value = nullable(num);
const subject: Parser<Subject> = (v, at) => {
	const r = record(v, at);
	return 'machine' in r
		? { machine: host(r.machine, `${at}.machine`) }
		: { service: service(r.service, `${at}.service`) };
};

const seriesData = tagged<SeriesData>({
	lines: obj({
		lines: list(obj<Line>({ name: str, points: list(tuple<[Ms, number | null]>(ms, value)) }))
	}),
	band: obj({
		points: list(tuple<[Ms, number | null, number | null, number | null]>(ms, value, value, value))
	})
});

const seriesShape = obj<Series>({
	of: subject,
	metric: oneOf('cpu', 'mem', 'disk', 'net', 'load', 'latency'),
	from: ms,
	to: ms,
	data: seriesData
});

const logLine = obj<LogLine>({
	at: ms,
	machine: host,
	unit: str,
	level: oneOf('error', 'warn', 'info', 'debug'),
	text: str
});

const timelineEntry = tagged<TimelineEntry>({
	pending: obj({ at: ms, node: host }),
	firing: obj({ at: ms, node: host }),
	resolved: obj({ at: ms, node: host }),
	notified: obj({ at: ms, by: host, resolved: bool })
});

const json = (text: string, at: string): unknown => {
	try {
		return JSON.parse(text);
	} catch {
		return fail(at, 'JSON');
	}
};

/** one message of `GET /api/stream` */
export const parseStreamEvent = (text: string) => streamEvent(json(text, 'stream'), 'stream');
/** one message of `GET /api/logs/tail` */
export const parseLogEvent = (text: string) =>
	tagged<LogEvent>({ lines: obj({ lines: list(logLine) }), expired: none } as Cases<LogEvent>)(
		json(text, 'tail'),
		'tail'
	);

export const parseSeries: Parser<Series> = seriesShape;
export const parseLogPage: Parser<LogPage> = obj<LogPage>({
	lines: list(logLine),
	before: nullable(str)
});
export const parseAlertDetail: Parser<AlertDetail> = obj<AlertDetail>({
	alert,
	timeline: list(timelineEntry),
	series: nullable(seriesShape)
});
export const parseAlerts: Parser<Alert[]> = list(alert);
export const parseHello: Parser<Hello> = obj<Hello>({ signin: list(str), build: str, schema: num });
export const parseSnapshot: Parser<Snapshot> = snapshotShape;
export const parseSignedIn: Parser<SignedIn> = obj<SignedIn>({
	token: str,
	viewer: obj<Viewer>({ login: str, device: str, expires_at: ms })
});
export const parsePushView: Parser<PushView> = obj<PushView>({
	key: str,
	subscribed: list(str),
	edit: nullable(str)
});
