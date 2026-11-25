# phoebe design system

phoebe sits next to coeus web on the Home Screen, so it speaks coeus's grammar: warm paper,
hairline rows, mono for anything a machine wrote, one accent. phoebe's identity is two things
only: a violet accent and the crescent mark. Everything else below is shared grammar, and
`scripts/design-gate.ts` fails the build on any value this file does not list.

The fenced `tokens`, `tokens-dark` and `scale` blocks are read by the gate. Edit them here
first, then in `src/lib/ui/tokens.css`.

## Color

Neutrals, ok, warn and bad are coeus's values. The accent moves from coeus blue (hue 262) to
violet (hue 295). The washes and `warn-ink` are mixes of those tokens, never new hues.

```tokens
--bg: #f6f6f3
--bg2: #ededea
--panel: #fbfbf9
--ink: #141413
--ink2: #4b4b47
--mute: #8b8b85
--faint: #b9b9b2
--rule: rgba(20, 20, 19, 0.1)
--rule2: rgba(20, 20, 19, 0.2)
--press: rgba(20, 20, 19, 0.045)
--acc: oklch(0.54 0.2 295)
--ok: oklch(0.6 0.13 155)
--warn: oklch(0.7 0.14 68)
--bad: oklch(0.57 0.2 27)
--warn-ink: color-mix(in oklab, var(--warn) 62%, var(--ink))
--acc-w: color-mix(in oklab, var(--acc) 11%, transparent)
--warn-w: color-mix(in oklab, var(--warn) 17%, var(--bg))
--bad-w: color-mix(in oklab, var(--bad) 12%, var(--bg))
--veil: color-mix(in oklab, var(--bg) 84%, transparent)
--scrim: rgba(20, 20, 19, 0.28)
```

```tokens-dark
--bg: #0b0b0c
--bg2: #151517
--panel: #0f0f11
--ink: #ececea
--ink2: #b1b1ab
--mute: #76766f
--faint: #46463f
--rule: rgba(255, 255, 255, 0.085)
--rule2: rgba(255, 255, 255, 0.17)
--press: rgba(255, 255, 255, 0.04)
--acc: oklch(0.74 0.15 295)
--ok: oklch(0.75 0.14 155)
--warn: oklch(0.8 0.13 75)
--bad: oklch(0.69 0.19 25)
--warn-ink: var(--warn)
--acc-w: color-mix(in oklab, var(--acc) 16%, transparent)
--warn-w: color-mix(in oklab, var(--warn) 15%, var(--bg))
--bad-w: color-mix(in oklab, var(--bad) 20%, var(--bg))
--veil: color-mix(in oklab, var(--bg) 84%, transparent)
--scrim: rgba(0, 0, 0, 0.55)
```

| token | role |
| --- | --- |
| bg, bg2, panel | page, pressed or chosen surface, docked bars |
| ink, ink2, mute, faint | text by rank: primary, secondary, labels, disabled and axis |
| rule, rule2 | hairlines, and the stronger hairline for control borders |
| press | the pressed state of anything tappable |
| acc, acc-w | the accent: chart lines, selection bar, links, the crescent; its wash fills chart areas and search matches |
| ok, warn, bad | status: up, warning, critical |
| warn-ink | warn as text; raw warn is too light to read on paper |
| warn-w, bad-w | the severity washes; they exist only for the severity treatment |
| veil | the translucent page color under the nav bar and the dock, with a blur |
| scrim | what dims the page under the log sheet |

Outside the three token blocks the app writes color only as `var(--token)`, `transparent`,
`currentColor` or `none`. A component may define a local custom property (`--s`, `--p`) only
from a token.

The mark is the moon's disc, a 2 unit ring, and the crescent it shows, in `acc`. The Home
Screen icon is that crescent alone, large, in `acc` on paper (`bg`), so it sits apart from
coeus's paper ring on an ink tile; `scripts/icons.ts` renders every icon file.

## Type

Two families, six sizes, three weights (400, 500, 600).

```scale
family: var(--f) var(--m)
font-size: 11px 13px 15px 17px 26px 34px
space: 0 2px 4px 8px 12px 16px 24px 32px 48px
radius: 0 8px 999px
rule: 1px 1.5px 2px 3px
```

- `--f` Instrument Sans: prose, titles, row names. `--m` Geist Mono: anything a machine wrote
  (hosts, units, numbers, times, log text) and every label. Mono always gets tabular figures.
- 11: section labels, kickers, axis ticks, table headers, meta beside a dot. Mono, upper case
  with 0.08em tracking when it is a label.
- 13: secondary text, mono values, log lines, table cells, sidebar items, controls.
- 15: body and row names.
- 17: band titles, nav and sheet titles.
- 26: a title that is an identifier (host, unit), in mono 500.
- 34: a title in prose, sans 600, tracking -0.03em.

## Space, radius, rules

- Spacing comes from the scale above, and its negatives for full-bleed (`margin: 0 -16px`).
  The phone gutter is 16. A desktop pane's gutter is 32.
- Two radii: 8 for anything you press that has a box (choices, the search field, the sheet's top
  corners, the chart tip), 999 for anything round (dots, pips). Rows, bands, tables and charts
  have no radius.
- Four rule weights: 1 hairline (rows, tables, label rules, control borders), 1.5 drawn data
  (chart lines, sparklines, probe map paths), 2 selection and gauges (the accent bar on a chosen row, a gauge's track and fill), 3
  severity (the bar of the severity treatment).

## Components

Each component lists its anatomy, then its states.

**Nav bar** (phone). 48px plus the safe area, `bg` at 84% with blur, hairline bottom once the
page scrolls. Left: the wordmark (crescent and "phoebe", 17/600) on the root, else a back or
close control with the parent's name. Center: the page title, shown once scrolled. Right: the
clock in mono 13 with a live dot, and on the root the search glyph and the account slot, which
opens this device: the first letter of the signed-in login, 11/600 `acc` on an `acc-w` disc
(radius 999), or the device glyph while the device holds no session. States: rest, scrolled, on-severity (it takes the severity
wash of the hero under it, so the wash reaches the top of the screen).

**Title block**. Kicker (11 mono label), title (34 sans or 26 mono), sub (15 ink2), meta (13 mono
mute with values in ink2), status line (dot and 15 ink2). States: plain; attention (the count
word of the fleet title in `bad`); clear (status dot ok).

**Section label**. `LABEL ───── value`: 11 mono mute label, a hairline that fills the gap, the
value in 13 mono ink2 or `bad`. 32 above, 8 below.

**Row**. A three-column grid: 7px dot, body, right. Body is the name (15/600, mono for
identifiers) over a sub line (13 ink2). Right is 13 mono ink2 and a chevron when it navigates.
12 above and below, hairline under; the dot sits on the name line. States: rest, pressed (`press`), selected (2px `acc` bar at
the leading edge), muted (asleep or off: name ink2, sub mute), affected (sub line in the
severity's ink: the thing is hurt by a firing alert but is not the alert).

**Status dot**. 7px circle. States: ok, warn, bad, acc (running), idle (faint), off (1.5 ring of
mute), asleep (ring with a bar), pulse (a firing critical: a ring of the dot's color expands
and fades, 1.6s). A **pip group** is four dots in a row, one per probing node: who sees it.

**Severity treatment**. The one way phoebe asks for attention, used for every firing alert and
nothing else: the fleet's alert bands, the machine's and service's alert line, the alert
screen's header, the sidebar's alert items. Anatomy: the `sev` wash (`bad-w` or `warn-w`)
across the full width, a 3px bar of `bad` or `warn` on the leading edge, a pulsing dot (critical)
or a still dot (warning), and a kicker in the severity's ink (`bad` or `warn-ink`):
`FIRING · PAGE · UNITFAILED`. The title stays `ink`. It comes in three scales built from
the same parts: **item** (sidebar: one line, 13), **band** (rows: kicker, 17 title, 13 meta, pip
group), **hero** (alert header: kicker, 34 title, meta, prose, with the wash under the nav bar).
States: critical, warning. An alert's severity picks its state: `page`, meant to reach a phone,
is critical; `info`, which only shows in the app, is warning. The kicker names the severity as
the alert does, `page` or `info`. Resolved is not a severity state: the same object drops the wash and
the bar and shows an ok dot with a mute `RESOLVED` kicker, so it reads as an ordinary row.

**Chart**. Section label (name, value now), then a plot 600 by 100 units stretched to the
column, `--h` tall. Line 1.5 `acc`, area `acc-w`, three hairline grid lines, y max in 11 mono top
right, four time ticks under it. States: line; line and area; two series (second in mute);
range band (min to max across nodes, median line); fired mark (dashed 1.5 `bad` vertical);
gap (a `bad-w` span where data is missing, a timeout); hover (1px crosshair, tip on `ink` with
`bg` text, radius 8). Inline forms: **gauge** (a 2px `rule` track with an `acc` fill to the value, `bad`
fill past 85%), **sparkline** (1.5 line, currentColor).

**Probe map**. The four probing nodes and the service, one path from each node to the service,
the latency now written on each path. Node: 7px ring of ink2 on `bg`, name in 13 mono, 28px hit
area. Service: a filled ink dot with its name in 13/600 and host in 11 mono mute. Path: 1.5
`faint`. States per path: ok; selected (2px `acc` bar weight, node filled `acc`, its name ink);
failing (dashed 1.5 `bad`, node ring `bad`, label `timeout` in `bad`); failing and selected
(node filled `bad`). Tapping a node selects it and the latency chart below follows. Phone lays
the nodes out in a row above the service; desktop in a column left of it.

**Timeline**. A list on a 1px `rule` spine. Each event is time (13 mono mute), a dot on the spine
(hollow for a plain event, `bad` for a failure, `ok` for recovery) and text (15 ink).

**Key-value list**. Rows of key (13 mute, 96px column) and value (15 ink, or 13 mono for machine
values), hairline between.

**Log line**. Time (13 mono faint), host (desktop only, mono ink2), unit (mono mute), text (mono
ink2). One size, 13. States: info, warn (text `warn-ink`), err (text `bad`), match (search hit
on `acc-w`), fresh (fades in once).

**Log sheet** (phone). A **dock** at the bottom of every phone screen but Logs: full width,
`panel` with blur, hairline top, 48px plus the safe area: live dot, "Logs" 15/600, context in 13
mono mute, an up chevron. Tapping it raises the **sheet**: grab handle, header (17/600 title, 13
mute sub, Done in `acc`), live log lines. Radius 8 on its top corners, a `rule2` hairline at its
edge instead of a shadow. States: docked, open.

**Desktop table**. Header in 11 mono labels with a hairline under it, rows 13 (names and numbers
in mono, numbers right-aligned), 8 above and below a cell, hairline under each row. Cells hold
dots, gauges, sparklines. States: rest, hover (`press`), selected (2px `acc` bar), muted (off or
asleep), affected (the cell that is hurt in `bad` or `warn-ink`).

**Sidebar item**. Dot, label (13, mono for identifiers), right meta (11 mono mute). 32 tall,
16 sides. States: rest, hover (`press`), current (`bg2` and ink label), and for firing alerts the
severity treatment at item scale.

**Choice**. The one control for both the time range (1h 6h 24h 7d) and filters (machines,
units): 13 mono, 32 tall, 12 sides, radius 8, 1px `rule` border. States: rest (ink2), pressed,
on (`bg2` fill, `rule2` border, ink). A run of choices scrolls sideways on phone.

**Button**. The one action a page asks for: full width, 48 tall, radius 8, 15/600, centered.
States: primary (`acc` fill, `bg` text), quiet (1px `rule2` border, ink text), pressed (`press`
over it), disabled (mute text).

**Address field**. Something to copy, a sign-in address: 40 tall, radius 8, 1px `rule2`
border, the value in 13 mono, `copy` on the right in 13 mono `acc`, which reads `copied` for a
moment after a tap.

**Probe line**. What the app is waiting on: a dot and 13 mono mute, `asking the tailnet…`,
`not on the tailnet · checked 3s ago`. Dot states: idle (asking), acc (signing in), bad
(refused).

**Sign-in screen**. Outside the frame, one column up to 420 wide: the wordmark, a title block
(34 sans title, 15 ink2 sub), then either three numbered steps (15 ink2, the number 11 mono
mute), the address field and the probe line, or, once signed in, a key-value list of the
session and the primary button back to the screen the app left. States: asking, signing in,
refused (with a quiet Try again), ended (the session ran out), signed in.

**Entry block**. Text to paste into a file: `bg2` panel, radius 8, 12 padding, 13 mono ink2,
wrapping anywhere, a `copy` control top right as on the address field; under it the command
that edits the file in 13 mono ink2 and a mute `#` comment saying what to do.

**Notification row**. The device's notifications on the device screen: a row (dot, name 15/600,
sub 13 ink2) with a choice on the right that turns them on or off, and an entry block under
it while the list still lacks this device. Dot states: idle (off), acc (asking, or subscribed
but not in the list), ok (on), warn (off in settings, or the subscription changed).

**Search field**. 40 tall, radius 8, 1px `rule2` border, search glyph, 15 input, live dot and
label on the right.

## Layout

- Phone: one column, 16 gutter, nav bar on top, dock at the bottom.
- Desktop (900 and up): three columns. Sidebar 264 (wordmark with the account slot
  on its right, Fleet, Logs, then alerts,
  machines, services, each under a section label), main (32 gutter, content up to 920), live
  tail 380 (the log lines of what main shows). The Logs page drops the tail and gives main the
  width. Dense data turns into desktop tables here and only here.
