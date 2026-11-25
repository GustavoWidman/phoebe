import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'vitest';
import { notice, target } from '../../service-worker/notice.ts';

const ORIGIN = 'https://phoebe.example.com';
/** a page alert's push exactly as the hook sends it, pinned by the hook's own test */
const SENT = readFileSync(new URL('../../../../contract/push.json', import.meta.url), 'utf8');

describe('notice', () => {
	test('shows what the hook sends, as it sends it', () => {
		expect(notice(SENT, ORIGIN)).toEqual({
			title: 'lavalink failed on oracle-1',
			body: 'UnitFailed, firing',
			navigate: '/alerts/5c1d0a5b3e8f2a71',
			tag: '5c1d0a5b3e8f2a71',
			timestamp: 1795632120000
		});
	});

	test('shows something for anything it cannot read', () => {
		const fallback = { title: 'phoebe', body: '', navigate: '/' };
		for (const text of [null, '', 'not json', 'null', '{}', '{"notification":{"body":"x"}}'])
			expect(notice(text, ORIGIN), String(text)).toEqual(fallback);
	});

	test('leaves out members of the wrong type', () => {
		const text = JSON.stringify({ notification: { title: 't', body: 3, tag: {}, timestamp: 'x' } });
		expect(notice(text, ORIGIN)).toEqual({
			title: 't',
			body: '',
			navigate: '/',
			tag: undefined,
			timestamp: undefined
		});
	});
});

describe('target', () => {
	test('is a path of the app, and the fleet for anything that leaves it', () => {
		expect(target(`${ORIGIN}/alerts/5c1d0a5b3e8f2a71`, ORIGIN)).toBe('/alerts/5c1d0a5b3e8f2a71');
		expect(target('/machines/lab', ORIGIN)).toBe('/machines/lab');
		expect(target('https://evil.example/alerts/x', ORIGIN)).toBe('/');
		expect(target('javascript:alert(1)', ORIGIN)).toBe('/');
		expect(target(undefined, ORIGIN)).toBe('/');
	});
});
