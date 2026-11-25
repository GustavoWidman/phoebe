import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, test } from 'vitest';
import {
	ParseError,
	parseAlertDetail,
	parseAlerts,
	parseHello,
	parseLogEvent,
	parseLogPage,
	parseSeries,
	parseStreamEvent
} from './parse.ts';

/** an example of every message the server sends, generated from its types */
const contract = JSON.parse(
	readFileSync(resolve(import.meta.dirname, '../../../../contract/web.json'), 'utf8')
) as Record<string, unknown>;

describe('every message of contract/web.json parses', () => {
	test('stream', () => {
		for (const m of contract.stream as unknown[])
			expect(parseStreamEvent(JSON.stringify(m))).toEqual(m);
	});
	test('tail', () => {
		for (const m of contract.tail as unknown[]) expect(parseLogEvent(JSON.stringify(m))).toEqual(m);
	});
	test('series', () => {
		for (const m of contract.series as unknown[]) expect(parseSeries(m, 'series')).toEqual(m);
	});
	test('logs, alerts, alert, hello', () => {
		expect(parseLogPage(contract.logs, 'logs')).toEqual(contract.logs);
		expect(parseAlerts(contract.alerts, 'alerts')).toEqual(contract.alerts);
		expect(parseAlertDetail(contract.alert, 'alert')).toEqual(contract.alert);
		expect(parseHello(contract.hello, 'hello')).toEqual(contract.hello);
	});
});

test('a message off the contract names where it went wrong', () => {
	const snapshot = (contract.stream as { kind: string }[]).find((m) => m.kind === 'snapshot')!;
	const bad = structuredClone(snapshot) as unknown as { machines: { state: { kind: string } }[] };
	bad.machines[0].state.kind = 'asleep';
	expect(() => parseStreamEvent(JSON.stringify(bad))).toThrow(
		new ParseError('stream.machines[0].state.kind: not up or down or resting')
	);
});
