/**
 * Photographs every screen and state of the built app against `phoebe web --fixture`, on a
 * phone (webkit, 390 by 844) and a desktop (chromium, 1440 by 900), light and dark, with the
 * fixture's clock moved to each state. Then lays each set out as a contact sheet.
 *
 *   bun scripts/shots.ts [--phoebe <binary>] [--out <dir>] [--only <state>]
 *
 * It starts its own server with the build in build/, holds its clock still, and drives one
 * browser at a time. Shots land in shots/<state>-<phone|desk>-<light|dark>.png, sheets in
 * shots/sheet-<phone|desk>-<light|dark>.png.
 */
import { spawn } from 'node:child_process';
import { mkdirSync, readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { chromium, webkit, type Browser, type Page } from '@playwright/test';

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

/** the fixture's clock: the timeline starts firing; from 18 minutes on it is all clear */
const FIRING = 0;
const CLEAR = 1200;

type Device = 'phone' | 'desk';
type State = {
	name: string;
	t: number;
	path: (alerts: Record<string, string>) => string;
	on?: Device[];
	act?: (page: Page) => Promise<void>;
};

const STATES: State[] = [
	{ name: 'fleet', t: FIRING, path: () => '/' },
	{ name: 'fleet-clear', t: CLEAR, path: () => '/' },
	{ name: 'machine', t: FIRING, path: () => '/machines/oracle-1' },
	{ name: 'machine-resting', t: FIRING, path: () => '/machines/laptop-mac' },
	{ name: 'service', t: FIRING, path: () => '/services/portfolio' },
	{
		name: 'service-tap',
		t: FIRING,
		path: () => '/services/portfolio',
		act: async (page) => {
			await page.locator('.map g[data-node="oracle-2"]').click();
			await settle(page);
		}
	},
	{ name: 'service-clear', t: CLEAR, path: () => '/services/portfolio' },
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
	{ name: 'device', t: FIRING, path: () => '/device' }
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
	const res = await fetch(`${BASE}/fixture/seek?t=${t}&speed=0`, { method: 'POST' });
	if (!res.ok) throw new Error(`seek: ${res.status}`);
}

/** the id of each alert of the snapshot, by its rule */
async function alertIds(): Promise<Record<string, string>> {
	const res = await fetch(`${BASE}/api/alerts?range=24h`);
	const alerts = (await res.json()) as { id: string; rule: string }[];
	return Object.fromEntries(alerts.map((a) => [a.rule, a.id]));
}

const server = spawn(
	PHOEBE,
	[
		'web',
		'--fixture',
		'--speed',
		'0',
		'--listen',
		`127.0.0.1:${PORT}`,
		'--web-dir',
		join(WEB, 'build')
	],
	{
		stdio: 'ignore'
	}
);
process.on('exit', () => server.kill());
for (let i = 0; ; i++) {
	try {
		await fetch(`${BASE}/api/hello`);
		break;
	} catch {
		if (i > 50) throw new Error(`${PHOEBE} did not start`);
		await new Promise((ok) => setTimeout(ok, 100));
	}
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
		const page = await context.newPage();
		for (const state of states) {
			if (state.on && !state.on.includes(device)) continue;
			await seek(state.t);
			await page.goto(`${BASE}${state.path(await alertIds())}`);
			await settle(page);
			await state.act?.(page);
			const file = join(OUT, `${state.name}-${device}-${scheme}.png`);
			await page.screenshot({ path: file });
			(shots[`${device}-${scheme}`] ??= []).push(file);
			console.log(file);
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
server.kill();
