import { describe, expect, test } from 'vitest';
import { precacheRequests } from './precache.ts';

describe('a build precaches from the network', () => {
	const requests = precacheRequests(
		['/', '/_app/immutable/index-abc.js'],
		'https://phoebe.example'
	);

	test('past the browser cache, which may hold an older page', () => {
		expect(requests.map((r) => r.cache)).toEqual(['reload', 'reload']);
	});

	test("at the worker's own origin", () => {
		expect(requests.map((r) => r.url)).toEqual([
			'https://phoebe.example/',
			'https://phoebe.example/_app/immutable/index-abc.js'
		]);
	});
});
