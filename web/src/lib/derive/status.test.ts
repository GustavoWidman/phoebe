import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { expect, test } from 'vitest';
import { parseStreamEvent } from '#lib/wire/parse.ts';
import type { Snapshot } from '#lib/wire/types.ts';
import { TREATMENT, alertDot, alertProse, alertTitle, headline, serviceStatus } from './status.ts';

const contract = JSON.parse(
	readFileSync(resolve(import.meta.dirname, '../../../../contract/web.json'), 'utf8')
) as { stream: unknown[] };
const snap = contract.stream
	.map((m) => parseStreamEvent(JSON.stringify(m)))
	.find((m) => m.kind === 'snapshot') as Snapshot;

test('a page alert takes the critical treatment, an info alert the warning one', () => {
	expect(TREATMENT).toEqual({ page: 'crit', info: 'warn' });
	const firing = snap.alerts.filter((a) => a.state.kind === 'firing');
	expect(firing.map((a) => [a.severity, alertDot(a)])).toEqual(
		firing.map((a) => [a.severity, a.severity === 'page' ? 'bad' : 'warn'])
	);
	for (const a of snap.alerts.filter((a) => a.state.kind === 'resolved'))
		expect(alertDot(a)).toBe('ok');
});

test('the fleet leads with what fires, in the words the rules give', () => {
	const head = headline(snap);
	expect(head.kind).toBe('firing');
	if (head.kind !== 'firing') return;
	expect(head.count).toBe(snap.alerts.filter((a) => a.state.kind === 'firing').length);
	for (const a of snap.alerts.filter((a) => a.state.kind === 'firing'))
		expect(head.say).toContain(alertTitle(a));
});

test('a node that cannot reach a service makes it warn, not fail', () => {
	const portfolio = snap.services.find((s) => s.probes.some((p) => p.views.some((v) => !v.ok)));
	expect(portfolio).toBeDefined();
	const st = serviceStatus(portfolio!, snap.nodes);
	expect(st.dot).toBe('warn');
	expect(st.reached!.ok).toBe(st.reached!.of - st.failing.length);
});

test('a summary that starts with an identifier keeps its spelling', () => {
	const a = snap.alerts[0];
	expect(alertProse({ ...a, summary: 'lavalink.service of lavalink failed on oracle-1' })).toBe(
		'lavalink.service of lavalink failed on oracle-1.'
	);
	expect(alertProse({ ...a, summary: 'the certificate expires in 8d' })).toBe(
		'The certificate expires in 8d.'
	);
});

test('a heartbeat that is late or missing fails its service; one nobody can read idles it', () => {
	const sync = snap.services.find((s) => s.heartbeat && !s.unit && !s.probes.length);
	expect(sync?.heartbeat?.state).toBe('fresh');
	expect(serviceStatus(sync!, snap.nodes)).toMatchObject({ dot: 'ok', stale: false });
	const as = (state: 'late' | 'missing' | 'unknown') =>
		serviceStatus({ ...sync!, heartbeat: { ...sync!.heartbeat!, state } }, snap.nodes);
	expect(as('late')).toMatchObject({ dot: 'bad', stale: true });
	expect(as('missing')).toMatchObject({ dot: 'bad', stale: true });
	expect(as('unknown')).toMatchObject({ dot: 'idle', stale: false });
});
