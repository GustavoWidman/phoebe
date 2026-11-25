import type { Page } from '@playwright/test';
import { expect, test } from './fleet.ts';

// a fleet that asks its readers to sign in, as a node does; the fixture signs in whoever asks
// on its own address, which stands in for the node's tailnet listener
test.use({ signin: true });

/** what the device keeps in IndexedDB */
const device = (page: Page) =>
	page.evaluate(async () => {
		const db = await new Promise<IDBDatabase>((ok) => {
			const r = indexedDB.open('phoebe-device', 1);
			r.onsuccess = () => ok(r.result);
		});
		const stored = await new Promise<{
			keys: CryptoKeyPair;
			session: { token: string; ended?: boolean } | null;
		}>((ok) => {
			const r = db.transaction('phoebe-device').objectStore('phoebe-device').get('this');
			r.onsuccess = () => ok(r.result);
		});
		return {
			extractable: stored.keys.privateKey.extractable,
			token: stored.session?.token ?? null,
			ended: stored.session?.ended ?? false
		};
	});

/** marks the session the device keeps as one the server no longer takes */
const end = (page: Page) =>
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

const title = (page: Page) => page.locator('.auth h1');

test('a device without a session signs itself in on the tailnet, then opens the fleet', async ({
	page,
	fleet: _
}) => {
	await page.goto('/');
	await expect(page).toHaveURL(/\/signin$/);
	await expect(title(page)).toHaveText('Signed in on the tailnet');
	await expect(page.locator('.auth .kv')).toContainText('gustavo@github');
	await expect(page.locator('.auth .kv')).toContainText('7 days');
	const stored = await device(page);
	expect(stored.extractable).toBe(false);
	expect(stored.token).toMatch(/^v1\./);
	await page.getByRole('link', { name: 'Open the fleet' }).click();
	await expect(page).toHaveURL(/\/$/);
	await expect(page.getByRole('heading', { level: 1 })).toHaveText('2 things need you.');
	await expect(page.locator('[data-account] .me').first()).toHaveText('G');
});

test('signing in returns to the alert a notification opened', async ({ page, fleet: _ }) => {
	await page.goto('/signin');
	await page.getByRole('link', { name: 'Open the fleet' }).click();
	const alert = await page.locator('a.band', { hasText: 'lavalink' }).first().getAttribute('href');
	expect(alert).toMatch(/^\/alerts\/[0-9a-f]{16}$/);
	await end(page);
	await page.goto(alert!);
	await expect(page).toHaveURL(/\/signin$/);
	await page.getByRole('link', { name: 'Open where you were' }).click();
	await expect(page).toHaveURL(new RegExp(`${alert}$`));
	await expect(page.locator('header.hero h1')).toHaveText('lavalink.service failed');
});

test('a session the server stops taking ends on the sign-in screen', async ({ page, fleet: _ }) => {
	await page.goto('/signin');
	await expect(title(page)).toHaveText('Signed in on the tailnet');
	await page.route('**/auth/ping', (r) => r.abort('connectionrefused'));
	await page.route('**/api/stream', (r) => r.fulfill({ status: 401, body: '' }));
	// the fleet opens on a session the device still holds, which the server refuses
	await page.goto('/');
	await expect(page).toHaveURL(/\/signin$/);
	await expect(title(page)).toHaveText('Your session ended');
	await expect(page.locator('.probe')).toContainText('not on the tailnet · checked');
	expect((await device(page)).ended).toBe(true);
});

test('a stream that says its session expired ends on the sign-in screen', async ({
	page,
	fleet: _
}) => {
	await page.goto('/signin');
	await expect(title(page)).toHaveText('Signed in on the tailnet');
	await page.route('**/auth/ping', (r) => r.abort('connectionrefused'));
	await page.route('**/api/stream', (r) =>
		r.fulfill({
			status: 200,
			headers: { 'content-type': 'text/event-stream' },
			body: 'event: expired\ndata: {"kind":"expired"}\n\n'
		})
	);
	await page.goto('/');
	await expect(page).toHaveURL(/\/signin$/);
	await expect(title(page)).toHaveText('Your session ended');
});

test('a device the tailnet policy keeps out is told so, and asks again only when told to', async ({
	page,
	fleet: _
}) => {
	let asked = 0;
	await page.route('**/auth/session', (r) => {
		asked++;
		return r.fulfill({ status: 403, body: 'the tailnet policy grants no access' });
	});
	await page.goto('/signin');
	await expect(title(page)).toContainText('may not sign in');
	await expect(page.locator('.probe')).toContainText('the tailnet policy grants no access');
	await page.waitForTimeout(3500);
	expect(asked).toBe(1);
	await page.getByRole('link', { name: 'Try again' }).click();
	await expect.poll(() => asked).toBe(2);
});

test('off the tailnet it keeps asking, and says when it last did', async ({ page, fleet: _ }) => {
	await page.route('**/auth/ping', (r) => r.abort('connectionrefused'));
	await page.goto('/signin');
	await expect(title(page)).toHaveText('Open this on your tailnet to sign in');
	await expect(page.locator('.addr')).toContainText('http://127.0.0.1:');
	await expect(page.locator('.probe')).toContainText('not on the tailnet · checked');
	await expect(page.locator('.steps li')).toHaveCount(3);
});
