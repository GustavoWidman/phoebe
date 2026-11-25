// generated from src/web/wire.rs by `PHOEBE_BLESS=1 cargo test web::`; change the rust types, not this file

/** epoch milliseconds */
export type Ms = number;
/** a machine's name in the registry, nodes included */
export type Hostname = string & { readonly __host: unique symbol };
/** a service's name in the registry */
export type ServiceName = string & { readonly __service: unique symbol };
/** an alert's alertmanager fingerprint, 16 hex digits */
export type AlertId = string & { readonly __alert: unique symbol };

/** `GET /api/hello`, the one route that answers before signing in, and it carries no fleet data */
export type Hello = {
  /** the serving nodes' sign-in addresses, reachable only on the tailnet */
  signin: Array<string>;
  /** the web app build this server serves */
  build: string;
  schema: number;
};

/** what a browser sends to `POST /auth/session` to sign in: its public key, and its signature over `phoebe-signin-v1\n<public origin>\n<ts>` by the matching private key */
export type SignIn = {
  key: Jwk;
  ts: Ms;
  /** `r || s`, base64url, as WebCrypto signs */
  sig: string;
};

/** a P-256 public key as a JSON Web Key, as WebCrypto exports it */
export type Jwk = {
  kty: string;
  crv: string;
  x: string;
  y: string;
};

/** what signing in on the tailnet answers: the session to send as `Authorization: Phoebe <token>`, and who it is for */
export type SignedIn = {
  token: string;
  viewer: Viewer;
};

/** who is signed in, on which device, until when */
export type Viewer = {
  /** the tailnet login */
  login: string;
  /** the tailnet node, by its MagicDNS name */
  device: string;
  expires_at: Ms;
};

/** `GET /api/push`: the key a browser subscribes with, and the devices pushes go to, each the first 16 hex digits of its endpoint's SHA-256 */
export type PushView = {
  /** the VAPID public key, base64url, the browser's `applicationServerKey` */
  key: string;
  subscribed: Array<string>;
  /** the command that edits the subscription list */
  edit: string | null;
};

/** one message of `GET /api/stream`. a snapshot is the whole fleet, sent on connect and again whenever it changes */
export type StreamEvent = { kind: "snapshot" } & Snapshot | { kind: "expired" };

/** the fleet at one moment, as one node sees it */
export type Snapshot = {
  schema: number;
  /** the moment it describes */
  at: Ms;
  /** the node whose phoebe built it, from whose view `nodes` is seen */
  served_by: Hostname;
  /** in registry order */
  nodes: Array<Node>;
  /** in registry order, nodes included */
  machines: Array<Machine>;
  /** in registry order */
  services: Array<Service>;
  /** firing, newest first, then resolved in the last 24 hours, newest first */
  alerts: Array<Alert>;
};

/** a node's stores, each up or not as `served_by` sees it */
export type Node = {
  name: Hostname;
  prometheus: boolean;
  loki: boolean;
  alertmanager: boolean;
  blackbox: boolean;
};

export type Machine = {
  name: Hostname;
  os: Os;
  /** a machine that is not always on rests instead of going down */
  always_on: boolean;
  /** as last scraped; unset until it first is */
  hardware: Hardware | null;
  state: MachineState;
};

export type Os = "nixos" | "darwin";

export type Hardware = {
  /** `aarch64`, `x86_64` */
  arch: string;
  cores: number;
  mem_bytes: number;
  /** the size of the root filesystem */
  disk_bytes: number;
};

/** whether the machine answers its scrapes */
export type MachineState =
  | { kind: "up"; since: Ms; booted_at: Ms; vitals: Vitals; units: Units }
  | { kind: "down"; since: Ms }
  | { kind: "resting"; since: Ms };

/** the machine's last scrape */
export type Vitals = {
  /** the fraction of cpu time spent working, over the last minute */
  cpu: number;
  /** the fraction of memory in use */
  mem: number;
  /** the fraction of the root filesystem in use */
  disk: number;
  load1: number;
};

/** the machine's systemd units, or its launchd jobs on darwin */
export type Units = {
  active: number;
  failed: Array<FailedUnit>;
};

export type FailedUnit = {
  name: string;
  since: Ms;
};

export type Service = {
  name: ServiceName;
  machine: Hostname;
  /** unset for a service with no unit of its own */
  unit: UnitStatus | null;
  /** in registry order */
  probes: Array<Probe>;
};

export type UnitStatus = {
  /** the systemd unit, or the launchd label on darwin */
  name: string;
  state: UnitState;
  /** when it entered `state`; unset while it is unknown */
  since: Ms | null;
};

export type UnitState = "active" | "activating" | "inactive" | "failed" | "unknown";

/** one probe of a service, as every node that answered runs it */
export type Probe = {
  kind: ProbeKind;
  /** a URL for http, `host:port` for the others, a server for dns */
  target: string;
  /** whether it speaks TLS from the first byte, as an https URL does */
  tls: boolean;
  /** the earliest expiry in the certificate chain it was served */
  cert_expires_at: Ms | null;
  /** one per node that answered, in registry order */
  views: Array<ProbeView>;
};

export type ProbeKind = "http" | "tcp" | "smtp" | "imap" | "dns";

/** the last probe one node ran */
export type ProbeView = {
  node: Hostname;
  ok: boolean;
  /** how long the probe took, up to its timeout when it failed */
  latency_ms: number;
  /** when `ok` last changed */
  since: Ms;
};

export type Alert = {
  id: AlertId;
  /** the rule's alertname */
  rule: string;
  severity: Severity;
  state: AlertState;
  /** when the first node fired it */
  starts_at: Ms;
  machine: Hostname | null;
  service: ServiceName | null;
  summary: string;
  /** the nodes whose prometheus fires it, or fired it once it resolved; a node that did not answer is left out */
  seen_by: Array<Hostname>;
  labels: { [key in string]: string };
};

export type Severity = "page" | "info";

export type AlertState = { kind: "firing" } | { kind: "resolved"; at: Ms };

/** `GET /api/alerts/<id>` */
export type AlertDetail = {
  alert: Alert;
  /** oldest first */
  timeline: Array<TimelineEntry>;
  /** the series that shows what fired it, around when it did */
  series: Series | null;
};

export type TimelineEntry =
  | { kind: "pending"; at: Ms; node: Hostname }
  | { kind: "firing"; at: Ms; node: Hostname }
  | { kind: "resolved"; at: Ms; node: Hostname }
  | { kind: "notified"; at: Ms; by: Hostname; resolved: boolean };

/** `GET /api/series`: one metric of one machine or service over a range */
export type Series = {
  of: Subject;
  metric: Metric;
  from: Ms;
  to: Ms;
  data: SeriesData;
};

export type Subject = { machine: Hostname } | { service: ServiceName };

/** a machine's `cpu`, `mem` and `disk` are fractions, `net` is bytes per second received (`rx`) and sent (`tx`), `load` the 1-minute load average; a service's `latency` is milliseconds, the slowest of its probes */
export type Metric = "cpu" | "mem" | "disk" | "net" | "load" | "latency";

/** points are `[t, value]`, or `[t, min, median, max]` across nodes; null where nothing was measured */
export type SeriesData =
  | { kind: "lines"; lines: Array<Line> }
  | { kind: "band"; points: Array<[Ms, number | null, number | null, number | null]> };

export type Line = {
  /** the metric, `rx` or `tx` for `net`, or the node a latency is from */
  name: string;
  points: Array<[Ms, number | null]>;
};

/** `GET /api/logs` */
export type LogPage = {
  /** newest first */
  lines: Array<LogLine>;
  /** passed back as `before`, the next page's older lines; unset at the end */
  before: string | null;
};

export type LogLine = {
  at: Ms;
  machine: Hostname;
  /** the unit that wrote it, or the launchd label or log file on darwin */
  unit: string;
  level: Level;
  text: string;
};

export type Level = "error" | "warn" | "info" | "debug";

/** one message of `GET /api/logs/tail` */
export type LogEvent = { kind: "lines"; lines: Array<LogLine> } | { kind: "expired" };

/** what `POST /fixture/seek` answers: where the simulated fleet's clock is */
export type FixtureClock = {
  /** seconds after the timeline starts */
  t: number;
  /** timeline seconds per real second; 0 holds the clock */
  speed: number;
  /** the moment it is in the simulated fleet */
  now: Ms;
};
