# phoebe

a lamp for a nix fleet, so i can see what's on fire.

phoebe is the observability app for my nix fleet. the part that exists is
everything under the app: nix modules that collect every machine's metrics
and logs, store them on several machines at once, and turn them into alerts,
all generated from one registry of the fleet's machines and services. the
`phoebe` binary serves the app's contract over a simulated fleet so far, for
the web app to be built against.

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
  alerting.webhook = "http://127.0.0.1:8080/alerts";
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

every machine writes the registry to `/etc/phoebe/fleet.json`, for the app to
draw from.

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
them to `alerting.webhook` in its webhook format. while `MachineDown` fires
for a machine, it holds back that machine's `ServiceUnreachable`, `UnitFailed`,
`NodeUnhealthy` and `CertExpiring`, so a machine going down is one
notification. that only works because `MachineDown` fires first: its scrapes
fail within 15 seconds and probes within a minute, and both rules wait 5
minutes.

## the app

`phoebe web` is the app the fleet is read through: a server next to every
node's stores, and a web app that installs on a phone. what exists of it so
far is its contract and a simulated fleet that serves it:

```bash
nix develop -c cargo run -- web --fixture   # http://127.0.0.1:7980
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
│   ├── checks.nix        # every check that isn't the package's own
│   ├── rules-check.nix   # generated config through promtool and amtool, and the rules' tests
│   ├── test.nix          # two nodes and an agent, booted in vms
│   ├── darwin-check.nix  # the nix-darwin module's daemons, evaluated on macos
│   └── modules/
│       ├── common.nix    # services.phoebe and the registry, for both platforms
│       ├── config.nix    # every program's configuration, generated from the registry
│       ├── nixos.nix     # nixpkgs' services and the firewall
│       └── darwin.nix    # the launchd daemons
├── contract/web.json     # an example of every message the app reads
├── web/src/lib/wire/     # their typescript types
└── src/
    ├── main.rs           # the binary
    └── web/
        ├── wire.rs       # every shape the app reads
        ├── query.rs      # what it asks, parsed at the edge
        ├── site.rs       # the routes
        ├── source.rs     # where their answers come from
        ├── fixture/      # the simulated fleet and its clock
        └── test/         # the contract, the generated types and the routes
```

## hacking on it

```bash
nix develop        # the toolchain
nix flake check    # the package, its tests and clippy, the rule tests and the
                   # nix-darwin module, plus the vm test on linux
```

the vm test boots two nodes and an agent on one network standing in for the
tailnet. it checks that both prometheus scrape all three machines, that a
line logged on the agent reaches both lokis, that lines logged while one loki
is stopped reach it once it starts again, that a failed unit reaches the
webhook exactly once from two alertmanagers, and that switching a node off
makes the other one fire `MachineDown` for it. it waits out the real rule
durations, so bring a coffee.

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

the binary only serves a fleet it made up so far. the lamp is wired and lit,
it's just pointed at a painting of the room.
