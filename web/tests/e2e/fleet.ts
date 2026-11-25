/**
 * A `phoebe web --fixture` of the worker's own, serving the built app, and the test's way to
 * move its clock. The fixture starts firing (t = 0) and is all clear from t = 1080 on.
 */
import { spawn, type ChildProcess } from 'node:child_process';
import { resolve } from 'node:path';
import { test as base } from '@playwright/test';

export const FIRING = 0;
export const CLEAR = 1200;

type Phoebe = {
	base: string;
	/** moves the clock to `t` seconds into the timeline, held still unless `speed` is given */
	seek(t: number, speed?: number): Promise<void>;
	/** the id of each alert the fixture knows, by its rule */
	alerts(): Promise<Record<string, string>>;
};

const BINARY = process.env.PHOEBE ?? resolve(import.meta.dirname, '../../../target/release/phoebe');
const BUILD = resolve(import.meta.dirname, '../../build');

export const test = base.extend<{ fleet: Phoebe }, { phoebe: Phoebe }>({
	phoebe: [
		async ({}, use, info) => {
			const port = 7700 + info.workerIndex;
			const base = `http://127.0.0.1:${port}`;
			const server: ChildProcess = spawn(
				BINARY,
				['web', '--fixture', '--speed', '0', '--listen', `127.0.0.1:${port}`, '--web-dir', BUILD],
				{ stdio: 'ignore' }
			);
			for (let i = 0; ; i++) {
				try {
					if ((await fetch(`${base}/api/hello`)).ok) break;
				} catch {}
				if (i > 100) throw new Error(`${BINARY} did not start`);
				await new Promise((ok) => setTimeout(ok, 100));
			}
			await use({
				base,
				async seek(t, speed = 0) {
					const res = await fetch(`${base}/fixture/seek?t=${t}&speed=${speed}`, { method: 'POST' });
					if (!res.ok) throw new Error(`seek: ${res.status}`);
				},
				async alerts() {
					const all = (await (await fetch(`${base}/api/alerts?range=7d`)).json()) as {
						id: string;
						rule: string;
					}[];
					return Object.fromEntries(all.map((a) => [a.rule, a.id]));
				}
			});
			server.kill();
		},
		{ scope: 'worker' }
	],
	fleet: async ({ phoebe }, use) => {
		await phoebe.seek(FIRING);
		await use(phoebe);
	},
	baseURL: async ({ phoebe }, use) => use(phoebe.base)
});

export { expect } from '@playwright/test';
