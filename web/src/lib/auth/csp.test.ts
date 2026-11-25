import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'vitest';

// the page's Content-Security-Policy, as index.html declares it
const policy = (() => {
	const html = readFileSync(new URL('../../../index.html', import.meta.url), 'utf8');
	const content = /http-equiv="Content-Security-Policy"\s+content="([^"]+)"/.exec(html)?.[1];
	if (!content) throw new Error('index.html declares no Content-Security-Policy');
	return new Map(
		content.split(';').map((d) => {
			const [name, ...sources] = d.trim().split(/\s+/);
			return [name, sources] as const;
		})
	);
})();

/** whether a `connect-src` host source allows `url`, for the forms the policy uses */
function allows(source: string, url: URL): boolean {
	const m = /^https:\/\/\*\.([^:/]+)(?::(\d+|\*))?$/.exec(source);
	if (!m) return false;
	const [, suffix, port = '443'] = m;
	return (
		url.protocol === 'https:' &&
		url.hostname.endsWith(`.${suffix}`) &&
		(port === '*' || (url.port || '443') === port)
	);
}

describe('the page lets sign-in reach the nodes', () => {
	// a node's sign-in address, on any port the module's signin.port can name
	for (const address of [
		'https://lab.tail4a3ea.ts.net:45731/auth/ping',
		'https://oracle-1.example.ts.net:8443/auth/session'
	]) {
		test(address, () => {
			const url = new URL(address);
			expect((policy.get('connect-src') ?? []).some((s) => allows(s, url))).toBe(true);
		});
	}
});
