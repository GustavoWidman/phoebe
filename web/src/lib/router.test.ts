import { expect, test } from 'vitest';

// the router module listens on window; parse and href are pure
globalThis.addEventListener ??= () => {};
const history = { state: null, replaceState() {}, pushState() {} };
Object.assign(globalThis, { history, location: { pathname: '/' } });
const { href, parse } = await import('./router.svelte.ts');

test('every screen round-trips through its path', () => {
	for (const route of [
		{ screen: 'fleet' },
		{ screen: 'machine', name: 'oracle-1' },
		{ screen: 'service', machine: 'oracle-2', name: 'portfolio' },
		{ screen: 'service', machine: 'oracle-1', name: 'a/b c' },
		{ screen: 'alert', id: '0dc6658a43bd5ed1' },
		{ screen: 'logs' },
		{ screen: 'device' }
	] as const)
		expect(parse(href(route))).toEqual(route);
});

test('a service lives under its machine, and nowhere else', () => {
	expect(href({ screen: 'service', machine: 'oracle-1', name: 'norn' })).toBe(
		'/machines/oracle-1/services/norn'
	);
	expect(parse('/machines/oracle-2/services/norn')).toEqual({
		screen: 'service',
		machine: 'oracle-2',
		name: 'norn'
	});
	expect(parse('/services/norn')).toBeNull();
});

test('a notification opens the alert it names', () => {
	expect(parse('/alerts/0dc6658a43bd5ed1/')).toEqual({ screen: 'alert', id: '0dc6658a43bd5ed1' });
	expect(parse('/nowhere')).toBeNull();
});
