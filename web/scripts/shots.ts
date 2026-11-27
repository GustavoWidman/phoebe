/**
 * Photographs every screen and state of the built app against `phoebe web --fixture`, on a
 * phone (webkit, 390 by 844) and a desktop (chromium, 1440 by 900), light and dark, with the
 * fixture's clock moved to each state. Then lays each set out as a contact sheet.
 *
 *   bun scripts/shots.ts [--phoebe <binary>] [--out <dir>] [--only <state>]
 *
 * It starts its own servers with the build in build/, one open to anyone and one that asks its
 * readers to sign in, for the sign-in and device states, holds their clocks still, and drives
 * one browser at a time. Shots land in shots/<state>-<phone|desk>-<light|dark>.png, sheets in
 * shots/sheet-<phone|desk>-<light|dark>.png.
 */
import { spawn } from 'node:child_process';
import { mkdirSync, readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { chromium, webkit, type Browser, type BrowserContext, type Page } from '@playwright/test';
import { stubPush, type PushStub } from '../tests/e2e/push.ts';

const WEB = resolve(import.meta.dirname, '..');
const args = process.argv.slice(2);
const flag = (name: string) => {
	const i = args.indexOf(`--${name}`);
	return i < 0 ? undefined : args[i + 1];
};
const PHOEBE = flag('phoebe') ?? resolve(WEB, '../target/release/phoebe');
const OUT = resolve(flag('out') ?? join(WEB, 'shots'));
const ONLY = flag('only');
const PORT = 7991;
const BASE = `http://127.0.0.1:${PORT}`;
/** the fixture that asks its readers to sign in */
const SIGNIN = `http://127.0.0.1:${PORT + 1}`;

/** the fixture's clock: the timeline starts firing; from 18 minutes on it is all clear */
const FIRING = 0;
const CLEAR = 1200;

type Device = 'phone' | 'desk';
type State = {
	name: string;
	t: number;
	path: (alerts: Record<string, string>) => string;
	on?: Device[];
	/** on the fixture that signs its readers in, the page first set up by `before` */
	signin?: boolean;
	before?: (page: Page) => Promise<unknown>;
	act?: (page: Page) => Promise<void>;
};

const NEW = 'https://web.push.apple.com/QGxnew-iphone';
const KEY =
	'BBgqG0VbQAPnBlmSzWD_uRkfTJJ7rNkfZc3HbxlUfhd8DBhqYt_DqIxDFK71gxU01Umj-1CqpRCOP0NQP_4183g';
const push = (stub: PushStub) => (page: Page) => page.addInitScript(stubPush, stub);
const offTailnet = (page: Page) => page.route('**/auth/ping', (r) => r.abort('connectionrefused'));
/** the device's session, marked as one the server no longer takes */
const ended = (page: Page) =>
	page.evaluate(async () => {
		const db = await new Promise<IDBDatabase>((ok) => {
			const r = indexedDB.open('phoebe-device', 1);
			r.onsuccess = () => ok(r.result);
		});
		const store = (mode: IDBTransactionMode) =>
			db.transaction('phoebe-device', mode).objectStore('phoebe-device');
		const stored = await new Promise<{ session: object }>((ok) => {
			const r = store('readonly').get('this');
			r.onsuccess = () => ok(r.result);
		});
		stored.session = { ...stored.session, ended: true };
		await new Promise((ok) => (store('readwrite').put(stored, 'this').onsuccess = ok));
	});

const STATES: State[] = [
	{ name: 'fleet', t: FIRING, path: () => '/' },
	{ name: 'fleet-clear', t: CLEAR, path: () => '/' },
	{ name: 'machine', t: FIRING, path: () => '/machines/oracle-1' },
	{ name: 'machine-resting', t: FIRING, path: () => '/machines/laptop-mac' },
	{ name: 'service', t: FIRING, path: () => '/machines/oracle-2/services/portfolio' },
	{
		name: 'service-tap',
		t: FIRING,
		path: () => '/machines/oracle-2/services/portfolio',
		act: async (page) => {
			await page.locator('.map g[data-node="oracle-2"]').click();
			await settle(page);
		}
	},
	{ name: 'service-clear', t: CLEAR, path: () => '/machines/oracle-2/services/portfolio' },
	{ name: 'alert-page', t: FIRING, path: (a) => `/alerts/${a.UnitFailed}` },
	{ name: 'alert-info', t: FIRING, path: (a) => `/alerts/${a.CertExpiring}` },
	{ name: 'alert-resolved', t: FIRING, path: (a) => `/alerts/${a.DiskAlmostFull}` },
	{ name: 'alert-page-resolved', t: CLEAR, path: (a) => `/alerts/${a.UnitFailed}` },
	{
		name: 'logs',
		t: FIRING,
		path: () => '/logs',
		act: async (page) => {
			await page.getByRole('searchbox').fill('lavalink');
			await page.waitForTimeout(600);
			await settle(page);
		}
	},
	{
		name: 'logsheet',
		t: FIRING,
		path: () => '/machines/oracle-1',
		on: ['phone'],
		act: async (page) => {
			await page.locator('.dock').click();
			await page.waitForSelector('.sheet .logs .l');
			await page.waitForTimeout(500);
		}
	},
	{ name: 'device', t: FIRING, path: () => '/device' },
	{ name: 'signed-in', t: FIRING, path: () => '/signin', signin: true },
	{ name: 'signin-asking', t: FIRING, path: () => '/signin', signin: true, before: offTailnet },
	{
		name: 'signin-refused',
		t: FIRING,
		path: () => '/signin',
		signin: true,
		before: (page) =>
			page.route('**/auth/session', (r) =>
				r.fulfill({ status: 403, body: 'the tailnet policy grants no access' })
			)
	},
	{
		name: 'signin-ended',
		t: FIRING,
		path: () => '/signin',
		signin: true,
		before: async (page) => {
			await page.goto(`${SIGNIN}/device`);
			await page.waitForSelector('[data-session]');
			await ended(page);
			await offTailnet(page);
		}
	},
	{
		name: 'device-off',
		t: FIRING,
		path: () => '/device',
		signin: true,
		before: push({ permission: 'default', subscription: null, subscribe: { endpoint: NEW } })
	},
	{
		name: 'device-uncommitted',
		t: FIRING,
		path: () => '/device',
		signin: true,
		before: push({ permission: 'default', subscription: null, subscribe: { endpoint: NEW } }),
		act: async (page) => {
			await page.getByRole('button', { name: 'Turn on' }).click();
			await page.waitForSelector('.entry');
			await page.locator('section.push').scrollIntoViewIfNeeded();
		}
	},
	{
		name: 'device-on',
		t: FIRING,
		path: () => '/device',
		signin: true,
		before: push({
			permission: 'granted',
			subscription: { endpoint: 'https://web.push.apple.com/QGxfixture-iphone', key: KEY },
			subscribe: { endpoint: NEW }
		})
	},
	{
		name: 'device-install',
		t: FIRING,
		path: () => '/device',
		signin: true,
		on: ['phone'],
		before: push({
			supported: false,
			ios: true,
			permission: 'default',
			subscription: null,
			subscribe: { endpoint: NEW }
		})
	}
];

/** waits for the screen, its charts and the fonts */
async function settle(page: Page) {
	await page.waitForSelector('.tb, .hero, .lh');
	await page.waitForFunction(
		() =>
			[...document.querySelectorAll('.ch')].every((c) => c.querySelector('svg')) &&
			document.fonts.status === 'loaded'
	);
	await page.waitForTimeout(300);
}

async function seek(t: number) {
	for (const base of [BASE, SIGNIN]) {
		const res = await fetch(`${base}/fixture/seek?t=${t}&speed=0`, { method: 'POST' });
		if (!res.ok) throw new Error(`seek: ${res.status}`);
	}
}

/** the id of each alert of the snapshot, by its rule */
async function alertIds(): Promise<Record<string, string>> {
	const res = await fetch(`${BASE}/api/alerts?range=24h`);
	const alerts = (await res.json()) as { id: string; rule: string }[];
	return Object.fromEntries(alerts.map((a) => [a.rule, a.id]));
}

const servers = [
	[BASE, []],
	[SIGNIN, ['--signin']]
].map(([base, extra]) =>
	spawn(
		PHOEBE,
		[
			'web',
			'--fixture',
			'--speed',
			'0',
			'--listen',
			(base as string).replace('http://', ''),
			'--web-dir',
			join(WEB, 'build'),
			...extra
		],
		{ stdio: 'ignore' }
	)
);
process.on('exit', () => servers.forEach((s) => s.kill()));
for (const base of [BASE, SIGNIN])
	for (let i = 0; ; i++) {
		try {
			await fetch(`${base}/api/hello`);
			break;
		} catch {
			if (i > 50) throw new Error(`${PHOEBE} did not start`);
			await new Promise((ok) => setTimeout(ok, 100));
		}
	}

/** a page signed in to the fixture that asks for it */
async function signedIn(context: BrowserContext) {
	const page = await context.newPage();
	await page.goto(`${SIGNIN}/signin`);
	await page.waitForSelector('.auth .kv');
	await page.close();
}

mkdirSync(OUT, { recursive: true });
const DEVICES: Record<Device, () => Promise<Browser>> = {
	phone: () => webkit.launch(),
	desk: () => chromium.launch()
};
const shots: Record<string, string[]> = {};
const states = STATES.filter((s) => !ONLY || s.name === ONLY);
for (const device of ['phone', 'desk'] as Device[]) {
	const browser = await DEVICES[device]();
	for (const scheme of ['light', 'dark'] as const) {
		const context = await browser.newContext({
			...(device === 'phone'
				? {
						viewport: { width: 390, height: 844 },
						deviceScaleFactor: 2,
						isMobile: true,
						hasTouch: true
					}
				: { viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 }),
			colorScheme: scheme,
			reducedMotion: 'reduce',
			timezoneId: 'America/Sao_Paulo',
			locale: 'en-US',
			serviceWorkers: 'block'
		});
		await signedIn(context);
		for (const state of states) {
			if (state.on && !state.on.includes(device)) continue;
			await seek(state.t);
			const page = await context.newPage();
			await state.before?.(page);
			const base = state.signin ? SIGNIN : BASE;
			await page.goto(`${base}${state.path(await alertIds())}`);
			await settle(page);
			if (state.name.startsWith('signin-')) await page.waitForTimeout(1500);
			await state.act?.(page);
			const file = join(OUT, `${state.name}-${device}-${scheme}.png`);
			await page.screenshot({ path: file });
			(shots[`${device}-${scheme}`] ??= []).push(file);
			console.log(file);
			await page.close();
			// a state that ended the session leaves the next to sign in again
			if (state.name === 'signin-ended') await signedIn(context);
		}
		await context.close();
	}
	await browser.close();
}

if (!ONLY) {
	// contact sheets: every state of one device and theme, side by side, labeled
	const browser = await chromium.launch();
	const page = await browser.newPage({ deviceScaleFactor: 1 });
	for (const [key, files] of Object.entries(shots)) {
		const phone = key.startsWith('phone');
		const cols = phone ? 7 : 4;
		const w = phone ? 390 : 720;
		const cells = files
			.map((f) => {
				const name = f.split('/').pop()!.replace(`-${key}.png`, '');
				const data = readFileSync(f).toString('base64');
				return `<figure><img src="data:image/png;base64,${data}"><figcaption>${name}</figcaption></figure>`;
			})
			.join('');
		await page.setViewportSize({ width: cols * (w + 16) + 16, height: 800 });
		await page.setContent(
			`<style>body{margin:0;padding:16px;background:#8a8a85;font:500 15px/1 ui-monospace,monospace;color:#fff;display:grid;grid-template-columns:repeat(${cols},${w}px);gap:16px}figure{margin:0}img{width:${w}px;display:block}figcaption{padding-top:8px}</style>${cells}`
		);
		const file = join(OUT, `sheet-${key}.png`);
		await page.screenshot({ path: file, fullPage: true });
		console.log(file);
	}
	await browser.close();
}
servers.forEach((s) => s.kill());
