# phoebe

a lamp for a nix fleet, so i can see what's on fire.

phoebe is the observability app for my nix fleet. underneath it are nix
modules that collect every machine's metrics and logs, store them on several
machines at once, and turn them into alerts, all generated from one registry
of the fleet's machines and services. on top, the `phoebe` binary serves a
web app over those stores, from every machine that keeps them, and pushes an
alert that pages to my phone.

named after phoebe, the titaness of the bright moon, who married coeus. coeus
deploys the fleet, so it seemed only fair that his wife keep an eye on it.

## why

one monitoring box is one more machine that can be down, and it's always down
on the night something else is too. phoebe has no such box. a handful of
machines, the nodes, each keep a full copy of everything, and any one of them
can be off while the others go on storing, answering and alerting.

it runs stock prometheus, loki and alertmanager underneath, headless, with no
grafana. the fleet describes itself once, and every scrape target, probe,
alert rule and log route follows from that description, so nobody hand-edits
a `prometheus.yml`.

## the registry

the flake exports `nixosModules.default` and `darwinModules.default`. every
machine imports one and gets the same registry:

```nix
# a module every machine imports
services.phoebe = {
  enable = true;
  fleet = {
    nodes = [ "oracle-1" "oracle-2" "lab" "altar" ];
    machines = {
      oracle-1 = { os = "nixos"; alwaysOn = true; address = "100.64.0.1"; };
      altar = { os = "darwin"; alwaysOn = true; address = "100.64.0.2"; };
      desktop = { os = "nixos"; alwaysOn = false; address = "100.64.0.3"; };
      # ...
    };
    services.heimdall = {
      machine = "oracle-1";
      unit = "heimdall.service"; # a launchd label on darwin
      probes = [ { kind = "http"; target = "https://heimdall.example.com"; } ];
      metrics = { port = 9200; path = "/metrics"; }; # or null
    };
  };
  web.enable = true; # on the nodes: the app, see below
};
```

- **`nodes`** are the machines that store and serve. a machine finds out it's
  a node by finding its own name (`services.phoebe.machine`, the host name by
  default) in the list.
- **`machines`** is every machine, nodes included. `address` is its tailnet
  IPv4 address, which every other machine uses to reach it. `alwaysOn = false`
  is for desktops: they show up as off, and never page for it.
- **`services`** is every service worth watching. `unit` is the full systemd
  unit name, or the launchd label on darwin. `probes` take `http` (a URL that
  answers 2xx), `tcp`, `smtp` and `imap` (a `host:port`, with `tls = true` for
  the ports that start with TLS), and `dns` (a server, plus the `query` it
  should answer).

every machine writes the registry to `/etc/phoebe/fleet.json`, with its own
name as `machine`, for the app to draw from.

## what runs where

every machine runs the agent:

- **node exporter**, with the systemd collector on nixos. on darwin, a
  launchd job writes the state of every `org.nixos.*` job, and of every job
  the registry names, into its textfile directory every 30 seconds, so a
  launchd daemon that dies alerts just like a failed unit.
- **vector**, reading the journal on nixos and `services.phoebe.logs.files`
  (`/var/log/*.log`) on darwin, where a file it hasn't seen before is read
  from its end and a file untouched for a day is skipped, so a mac sends what
  its daemons write from the day it joins. it sends every line to every node's loki, one
  sink per node, and each sink has a disk buffer of its own
  (`logs.bufferSize`, 512 MiB). a node that's down holds up nothing but its
  own backlog, and gets all of it once it's back. a buffer that fills drops
  the newest lines for that node only. on nixos each line is labelled with
  its machine, its level and its unit, where instances of one unit share a
  label: every `session-<n>.scope` is `session.scope`, every
  `coeus-job-<n>.service` is `coeus-job.service`, every `user@<uid>.service`
  is `user@.service`.

every node also runs:

- **prometheus**, which scrapes every machine's node exporter and vector,
  every service's metrics, and every node's prometheus, loki, alertmanager
  and blackbox exporter. it also asks every node's blackbox exporter to run
  every probe, so each prometheus holds every node's view of every service.
  that makes a target see the square of the node count in probes, so probes
  run once a minute (16 a minute per target with four nodes) while every
  other scrape runs every 15 seconds. it keeps metrics for
  `node.retention.metrics` (90 days).
- **loki**, keeping logs for `node.retention.logs` (30 days). every machine
  pushes as one tenant, so its limits (16 MB/s, bursts of 32, 20,000 active
  streams) are the whole fleet's.
- **alertmanager**, in one gossip cluster with the other nodes. every
  prometheus sends its alerts to every alertmanager, and the cluster makes
  sure each notification goes out from one of them only.
- **the blackbox exporter**, which runs the probes.

on nixos these are nixpkgs' own services, fed phoebe's generated
configuration. they listen on every interface, and the firewall opens their
ports on `services.phoebe.interface` (`tailscale0`) alone. nix-darwin has no
modules for the stores, so on darwin phoebe runs them as launchd daemons of
its own, as a hidden `_phoebe` user, and every program binds to the tailnet
address and nothing else. a mac on a home lan has no business sharing its
metrics with the smart fridge.

| port | what | where |
| --- | --- | --- |
| 9100 | node exporter | every machine |
| 9598 | vector's own metrics | every machine |
| 9090 | prometheus | nodes |
| 3100 | loki | nodes |
| 9093 | alertmanager | nodes |
| 9094 | alertmanager gossip, TCP and UDP | nodes |
| 9115 | blackbox exporter | nodes |

## alerts

every rule carries a `severity`: `page` is meant to reach a phone, `info`
only shows up in the app.

| alert | severity | fires when |
| --- | --- | --- |
| `MachineDown` | page | an `alwaysOn` machine's node exporter is unreachable for 5 minutes |
| `UnitFailed` | page | a registry service's unit or launchd job has failed for a minute |
| `UnitFailed` | info | any other systemd unit, or `org.nixos.*` job, has failed for a minute |
| `ServiceUnreachable` | page | at least two nodes see the same probe fail for 5 minutes (one, in a fleet of one node) |
| `CertExpiring` | info | a probed certificate expires within 14 days |
| `DiskAlmostFull` | page | a writable filesystem is under 10% free for 15 minutes. macos's helper volumes (Preboot, Update, VM and the like) share their container's space with the data volume, so only the data volume counts |
| `DiskFillingFast` | info | a filesystem under 25% free will be full within a day, at the last 6 hours' rate |
| `NodeUnhealthy` | page | a node's prometheus, loki, alertmanager or blackbox exporter is down for 5 minutes while its machine is up |
| `LogsBacklog` | info | a machine's buffer for one node is over half full for 15 minutes |

alertmanager groups notifications by alert, machine and service, and posts
them to `alerting.webhook` in its webhook format. on a node running the app,
that's the app's hook unless set otherwise. while `MachineDown` fires
for a machine, it holds back that machine's `ServiceUnreachable`, `UnitFailed`,
`NodeUnhealthy` and `CertExpiring`, so a machine going down is one
notification. that only works because `MachineDown` fires first: its scrapes
fail within 15 seconds and probes within a minute, and both rules wait 5
minutes.

## the app

`phoebe web` is the app the fleet is read through: a server next to every
node's stores, and a web app that installs on a phone. each node runs its
own, and each reads its own node's prometheus, loki and alertmanager, so any
one node up is enough to see the fleet.

```nix
# on every node
services.phoebe.web = {
  enable = true;
  origin = "https://phoebe.example.com";
  push = {
    vapidKeyFile = "/run/agenix/phoebe-vapid";
    subscriptionsFile = "/run/agenix/phoebe-push-subscriptions";
  };
};
```

on nixos that's a `phoebe-web` systemd service, as a user of its own, in a
sandbox with nothing to write to. on darwin it's a launchd daemon running as
`_phoebe`, logging to `/var/log/phoebe-web.log`. either way it runs:

```bash
phoebe web --fleet /etc/phoebe/fleet.json \
  --listen 127.0.0.1:7980 --hook 127.0.0.1:7981 --web-dir <phoebe-web> \
  --origin https://phoebe.example.com \
  --push-key-file /run/agenix/phoebe-vapid \
  --push-subscriptions-file /run/agenix/phoebe-push-subscriptions
```

both listen on loopback alone. the app has no sign-in, so only the machine
itself gets to reach it. the hook listens apart from the app, so it stays on
loopback whatever address the app is given. the push key and the
subscription list must be readable by the app's user, `phoebe-web` on nixos
and `_phoebe` on darwin (agenix's `owner`).

### where the answers come from

every 15 seconds a task asks the node's prometheus for every series of the
snapshot at once, the alertmanager for what the cluster fires, and every
node's prometheus, over the tailnet with two seconds to answer, which of
those alerts it fires itself. it builds one snapshot of the fleet from that
and sends it to every open stream, unless nothing but the clock changed. a
node that doesn't answer in time is left out of who sees an alert, not
counted as not seeing it.

- **machines** are up while their node exporter answers. one that doesn't is
  down if it's `alwaysOn`, and resting if it isn't. since when comes from the
  last minute it did the other thing within a week. vitals are the last
  scrape, hardware is the last scrape within a week, so a resting desktop
  keeps its specs.
- **units** come from the systemd collector on nixos and the launchd
  reporter on darwin, read as the same states. a failed unit fails from when
  its `UnitFailed` went pending.
- **probes** are one view per node, from `probe_success` and
  `probe_duration_seconds` labelled with the node that ran them.
- **alerts** are the cluster's firing ones, then those that resolved in the
  last day, which come from this node's `ALERTS` series. an alert's id is
  alertmanager's fingerprint, which phoebe works out from an `ALERTS`
  series' labels the same way, so both name the same alert.

every PromQL phoebe sends lives in one table, `src/web/live/promql.rs`:

| series | PromQL, roughly |
| --- | --- |
| up, last up, last down | `up{job="node"}`, and `max_over_time(timestamp(up == 1 or 0)[7d:1m])` |
| boot, arch, cores, memory, disk size | `node_boot_time_seconds`, `node_uname_info`, `node_cpu_seconds_total`, `node_memory_*`, `node_filesystem_size_bytes{mountpoint="/"}`, the last within a week |
| cpu | the working modes' rate over every mode's, by machine |
| mem | `MemAvailable` over `MemTotal` on linux, app, wired and compressed memory over the total on darwin |
| disk, load | `/`'s free over its size, `node_load1` |
| rx, tx | `node_network_*_bytes_total` of the machine's own interfaces, no tunnels or bridges |
| units | `node_systemd_unit_state` and `phoebe_launchd_job_failed`, `ALERTS_FOR_STATE{alertname="UnitFailed"}` |
| stores | `up{job=~"prometheus\|loki\|alertmanager\|blackbox"}` |
| probes | `probe_success`, `probe_duration_seconds`, `probe_ssl_earliest_cert_expiry` |
| latency | the slowest probe of a service from each node, in ms, unless one fails |
| alerts | `ALERTS`, `ALERTS{alertstate="firing"}` and its `max_over_time` |

the other routes ask when a browser does. a chart is a range query of about
240 points. a log page is loki's `query_range` over the labels the data
plane sets (`machine`, `unit` and `level` from the journal, `machine` and
`file` from a darwin log file), and the tail bridges loki's tail websocket.
the journal files most programs' lines at info whatever they say, so a line
that names its own level (`level=warn`, `WARN`, `[error]`) is shown at that
one. an alert's timeline is every node's `ALERTS` of that alert over its
episode, plus who pushed it.

### pushes

alertmanager posts every notice to `POST /hook` on the hook's listener. for
each alert of a notice phoebe logs one line, `notice {"alert":...}` with the
alert's id, whether it resolved, its summary, and how many devices took the
push. vector ships that line to every node's loki like any other, which is
how every node's timeline shows who pushed what without asking its peers.

only `severity = page` alerts push. phoebe reads the VAPID key and the
subscription list at every send, so editing them needs no restart, and
sends a declarative web push through gjallar: the summary as the title,
`<origin>/alerts/<id>` as the place a tap opens, and the alert's id as the
tag, so the resolution replaces the firing notice on the phone. the cluster
sends each notice from one node, so one phoebe pushes it. one that can't read
its key or its list answers 500, alertmanager tries again, and another node
sends it instead.

### the simulated fleet

`phoebe web --fixture` serves the same routes over a simulated fleet, for
working on the web app with no fleet at all:

```bash
# the server, the simulated fleet and the built web app on http://127.0.0.1:7980
nix build .#phoebe-web -o result-web
nix develop -c cargo run --release -- web --fixture --web-dir result-web

# or the web app from source, with hot reload on http://127.0.0.1:5173, against
# a fixture running on 7980
cd web && bun install && bun run dev
```

the simulated fleet is the real one, with the same six machines, sixteen
services and their probes. its clock starts at 15:42 on 2026-11-25 with
lavalink failed on oracle-1 (a page), derp's certificate about to expire
(info), a full disk on oracle-2 resolved two hours earlier, lab unable to
reach r3dlust.com while every other node can, and both desktops resting.
over the next 18 minutes lab's route comes back, lavalink is restarted, derp
renews its certificate and the laptop wakes. from 16:00 on it's all clear.

every answer is a function of the moment alone, so a moment looks the same
however the clock got there. `--speed` sets how fast the clock runs (`0`
holds it), and `POST /fixture/seek?t=<seconds>[&speed=<x>]` moves it, which
is what tests do. `--web-dir` serves a built web app beside the api. anyone
who reaches the listening address gets in: it's a made-up fleet, it has
nothing to hide.

the routes, from either:

| route | answers |
| --- | --- |
| `GET /api/hello` | the build, the schema version and the sign-in addresses |
| `GET /api/stream` | server-sent events: the whole fleet on connect, then again at each scrape that changes it |
| `GET /api/series?of=machine:<name>&metric=<m>&range=<r>` | `[t, value]` points of `cpu`, `mem`, `disk`, `load`, or `net` as `rx` and `tx`, over `1h`, `6h`, `24h` or `7d` |
| `GET /api/series?of=service:<name>&metric=latency&range=<r>[&node=<node>]` | `[t, min, median, max]` across the nodes, or one node's `[t, value]` |
| `GET /api/logs?machine=&unit=&service=&q=&before=&limit=` | lines, newest first, and the `before` of the next page |
| `GET /api/logs/tail?machine=&unit=&service=&q=` | server-sent events: the lines that arrive |
| `GET /api/alerts?range=<r>` | the alerts that fired within the range (24h unless given), newest first |
| `GET /api/alerts/<id>` | an alert, each node's pending, firing and resolved, who pushed it, and the series that shows it |
| `POST /hook` | alertmanager's notices, on the hook's own loopback listener; not served by the fixture |

### the web app

`web/` is a svelte 5 app built by vite, with bun as the package manager and
`bun2nix` turning its lockfile into the `phoebe-web` package. it opens one
`/api/stream` and keeps the latest snapshot in one store that every screen
reads. series, log pages and alert details come from the other routes when a
screen asks. every answer is checked against the generated types where it
arrives, so nothing past the parser second-guesses its data.

it has five screens, each its own url, so a notification for an alert opens
straight onto `/alerts/<id>`:

- **the fleet**: while anything fires, "2 things need you." and a severity band
  per alert. once nothing does, a calm title and an all clear. then every
  machine with its vitals (a desktop that's off shows as resting, not down),
  every service with one dot per node that probes it, and what resolved in
  the last day.
- **a machine**: cpu, memory, disk, network and load over 1h, 6h, 24h or 7d,
  with a dashed line where its alert fired, then its units and its logs.
- **a service**: the probe map, where tapping a node charts that node's
  latency, a row per node, the latency band across all of them, its unit and
  its logs.
- **an alert**: the severity header across the full width, the nodes that see
  it, its timeline, its details, the series that shows it and its logs. a
  resolved alert keeps the screen and drops the severity.
- **logs**: a search and a machine filter over the live tail.

on a phone each screen has a nav bar on top and a log dock at the bottom,
which raises a sheet with the screen's logs. from 900px wide it's three
columns: a sidebar of every alert, machine and service, the screen, and the
live tail of the screen's logs, with tables for the long lists. `/device` is
this device's theme (system, light or dark), and the account slot in the
header leads there.

it installs on a home screen as a violet crescent. a service worker keeps a
copy of the build and answers every navigation from it, so the app opens
without the network and waits for a node to answer. `phoebe web` sends the
`.br` or `.gz` the build writes beside every text file to a browser that takes
it.

### the design system

`web/design/system.md` is the design system: the colors of both themes, six
font sizes in two families and three weights, a spacing scale, two radii and
four rule weights, then every component and its states. an alert asks for
attention one way only, with a severity treatment: a wash across the full
width, a 3px bar on its leading edge and a kicker in the severity's color. a
`page` alert, meant to reach a phone, gets the critical treatment (red, with a
pulsing dot). an `info` alert, which only shows in the app, gets the warning
one (amber, with a still dot). the kicker says which: `FIRING · PAGE ·
UNITFAILED`.

`web/scripts/design-gate.ts` holds the build to it. it reads the built css,
as the minifier wrote it, and the markup of every component, and fails on any
color, font size, family, weight, radius, spacing or rule weight the system
doesn't list. the token blocks must match the system's word for word, give or
take how the minifier spells a color. a value off the scale fails ci, so the
app can't drift one hex code at a time.

### checks and budgets

```bash
# unit tests: the parser over every message of contract/web.json, what the
# screens derive from the fleet, and the design gate's own tests
cd web && bun run test

# types and formatting
bun run check && bun run lint

# the design gate and the budgets, over a fresh build
bun run build && bun scripts/design-gate.ts && bun scripts/budget.ts

# the end-to-end tests: builds phoebe and the app, then drives every screen on
# a webkit phone (390x844) and a chromium desktop (1440x900)
nix develop -c e2e

# every screen and state, light and dark, on both, as contact sheets in web/shots
nix develop -c shots
```

the end-to-end tests run one browser at a time, each against a `phoebe web
--fixture` of its own, and move its clock to the state they test: the fleet
firing and all clear, an alert opened as a notification opens it, an alert
resolving while it's open, the machine's ranges, the probe map, log search
and the live tail, the log sheet, the desktop's columns, the theme, and the
shell opening offline.

each screen is a chunk of its own, and `web/perf-budget.json` caps what a cold
start on each route loads before its screen draws, gzipped: the entry, the
screen's chunk and everything they import.

| route | js | css |
| --- | --- | --- |
| `/` | 36 KiB | 5 KiB |
| `/machines/<name>` | 36 KiB | 5 KiB |
| `/services/<name>` | 38 KiB | 5 KiB |
| `/alerts/<id>` | 36 KiB | 5 KiB |
| `/logs` | 31 KiB | 5 KiB |
| `/device` | 30 KiB | 5 KiB |

about 16 KiB of each is svelte's runtime, which every route shares, and the
service screen's probe map and sparklines make it the heaviest. a route
that outgrows its line fails the check, so a heavier screen has to say so in
the budget file.

`nix flake check` builds the app and runs all of these but the end-to-end
tests and the shots, which want a browser.

### the contract

every shape the app reads is a rust type in `src/web/wire.rs`.
`web/src/lib/wire/types.ts` declares them for the browser, and
`contract/web.json` holds an example of every message, both generated from
those types. a test fails while either file differs from what the types
generate, and `PHOEBE_BLESS=1 cargo test web::` rewrites both. another test
fails while a kind the types declare shows up in no example, so the contract
can't quietly fall behind the types.

## sharp edges

- **addresses are written down, not discovered.** every machine reaches the
  others by the `address` in the registry. a machine that rejoins the tailnet
  with a new address stays unreachable until the registry says so.
- **darwin's daemons start before the tailnet does.** a program bound to the
  tailnet address exits until tailscaled brings the address up, and launchd
  starts it again every 10 seconds until then. `/var/log/phoebe-<name>.log`
  says so, loudly.
- **the app has no sign-in.** it listens on loopback alone until it does,
  so reaching it means being on the node, or tunnelling to it.
- **a resolved alert's summary can be lost.** `ALERTS` keeps labels, not
  annotations. phoebe remembers the summary of every alert it saw fire, and
  looks up the notice line of one it didn't. an alert alertmanager held
  back, so never sent, that resolved before phoebe restarted shows as its
  rule and its machine.
- **a chart starts where the node's prometheus does.** a node that lost its
  data, or joined last week, draws nothing before then, the same as a gap.
- **the firewall is the only fence on nixos.** the programs listen on every
  interface there, so a machine with `networking.firewall.enable = false`
  serves its metrics to whoever asks.
- **a crash-looping launchd job flickers.** the reporter counts a job as
  failed only while it isn't running, so one that launchd restarts every 10
  seconds can look healthy half the time.
- **the tailnet has to allow the ports.** the table above, from every machine
  to every node, and from every node to every machine.

## file structure

```text
.
├── .github/              # checks for every pull request, and the release
├── flake.nix             # the package, the modules, the checks and a dev shell
├── nix/
│   ├── package.nix       # the crane build, with the tests and clippy as checks
│   ├── web.nix           # the web app's build, with its tests, gate and budgets as checks
│   ├── checks.nix        # every check that isn't the package's own
│   ├── rules-check.nix   # generated config through promtool and amtool, and the rules' tests
│   ├── test.nix          # two nodes and an agent, booted in vms
│   ├── fake-push.py      # the push service the vm test pushes to
│   ├── darwin-check.nix  # the nix-darwin module's daemons, evaluated on macos
│   └── modules/
│       ├── common.nix    # services.phoebe and the registry, for both platforms
│       ├── config.nix    # every program's configuration, generated from the registry
│       ├── nixos.nix     # nixpkgs' services and the firewall
│       ├── darwin.nix    # the launchd daemons
│       └── web/          # services.phoebe.web, and its service on each platform
├── contract/web.json     # an example of every message the app reads
├── web/
│   ├── design/system.md  # the design system, which the design gate holds the build to
│   ├── scripts/          # the design gate, budgets, compression, icons and shots
│   ├── src/lib/
│   │   ├── wire/         # the typescript types, generated, and the parser
│   │   ├── stream/       # the fleet store and the event stream under it
│   │   ├── derive/       # what the screens show, worked out from the fleet
│   │   ├── ui/           # the tokens, the stylesheet and the components
│   │   ├── shell/        # the frame: sidebar and tail, or nav bar and log sheet
│   │   └── screens/      # fleet, machine, service, alert, logs and device
│   ├── src/service-worker/  # keeps the app opening offline
│   ├── static/           # the manifest and the icons
│   └── tests/e2e/        # the end-to-end tests
└── src/
    ├── main.rs           # the binary
    └── web/
        ├── wire.rs       # every shape the app reads
        ├── query.rs      # what it asks, parsed at the edge
        ├── site.rs       # the routes
        ├── source.rs     # where their answers come from
        ├── live/         # the real fleet: the stores' clients, the series table,
        │                 # the snapshot, the alerts, the hook and recorded answers
        ├── fixture/      # the simulated fleet and its clock
        └── test/         # the contract, the generated types and the routes
```

## hacking on it

```bash
nix develop        # the toolchain
nix flake check    # the package, its tests and clippy, the web app and its checks,
                   # the rule tests and the nix-darwin module, plus the vm test on linux
nix develop -c lock-web   # regenerate web/.bun.nix after changing web/bun.lock
```

the vm test boots two nodes and an agent on one network standing in for the
tailnet, with the app on both nodes and a push service on each node's
loopback. it checks that both prometheus scrape all three machines, that
each app streams every machine up and every probe from both nodes, that a
line logged on the agent reaches both lokis, that lines logged while one loki
is stopped reach it once it starts again, that a failed unit fires in both
apps seen by both nodes and pushes exactly once, signed and encrypted, that
both apps' timelines show who pushed it, and that switching a node off makes
the other one fire `MachineDown` for it and push that. it waits out the real
rule durations, so bring a coffee.

the source's unit tests parse answers recorded from a real node's stores, in
`src/web/live/test/recorded`. to record them again from a node:

```bash
PHOEBE_RECORD=/etc/phoebe/fleet.json nix develop -c cargo test record -- --ignored
```

every pull request runs `nix flake check` on x86_64-linux, kvm and vm test
included, and builds every other check on aarch64-linux and aarch64-darwin.
commits follow conventional commits, and a pull request raises the version in
`Cargo.toml` unless it's labeled `no-release`.

a push to `main` that brings a version with no release yet builds phoebe for
x86_64-linux, aarch64-linux and aarch64-darwin, pushes it to
`https://cache.r3dlust.com` (key
`cache.r3dlust.com-1:Wdkf9mes2ued0P7C7CL3Jax1b4KPuJwQpM3h4G/Z+yI=`) pinned as
`phoebe-<system>`, then tags it `v<version>` and releases it. when a pull
request already passed its check on the same tree, the release reuses that
check rather than sitting through the vm test again. a push that releases
nothing builds nothing. the modules put phoebe in every machine's system
packages, so a fleet should take a release tag as its input and keep the
input's own `nixpkgs` (no `follows`): then every machine gets that exact build
from the cache and none of them compiles rust.

---

the lamp is lit and pointed at the room. the first thing it found was a
laptop failing a job nobody had noticed.
