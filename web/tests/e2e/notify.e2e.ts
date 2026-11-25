import { readFileSync } from 'node:fs';
import type { Page } from '@playwright/test';
import { expect, test } from './fleet.ts';

/** a page alert's push exactly as the hook sends it, pinned by the hook's own test */
const SENT = readFileSync(new URL('../../../contract/push.json', import.meta.url), 'utf8');
const SENT_ID = '5c1d0a5b3e8f2a71';

// Playwright runs service workers in Chromium alone, and only Chromium's DevTools protocol
// delivers a push. The headless shell denies every notification, so these run in Chromium
// itself, headless.
test.skip(({ browserName }) => browserName !== 'chromium');
test.use({ serviceWorkers: 'allow', channel: 'chromium' });

/** the app open on the fleet, its service worker in charge, allowed to notify */
async function installed(page: Page, base: string) {
	await page.context().grantPermissions(['notifications'], { origin: base });
	await page.goto('/');
	await page.evaluate(() => navigator.serviceWorker.ready.then(() => undefined));
	await expect(page.getByRole('heading', { level: 1 })).toHaveText('2 things need you.');
}

/** delivers `data` to the service worker as the push service would */
async function deliver(page: Page, base: string, data: string) {
	const cdp = await page.context().newCDPSession(page);
	const registration = new Promise<string>((ok) =>
		cdp.on('ServiceWorker.workerRegistrationUpdated', ({ registrations }) => {
			const mine = registrations.find((r) => r.scopeURL === `${base}/` && !r.isDeleted);
			if (mine) ok(mine.registrationId);
		})
	);
	await cdp.send('ServiceWorker.enable');
	await cdp.send('ServiceWorker.deliverPushMessage', {
		origin: base,
		registrationId: await registration,
		data
	});
}

const shown = (page: Page) =>
	page.evaluate(async () => {
		const reg = await navigator.serviceWorker.ready;
		return (await reg.getNotifications()).map((n) => ({
			title: n.title,
			body: n.body,
			tag: n.tag,
			data: n.data
		}));
	});

test('a page alert shows as the hook sent it, its link a path of the app', async ({
	page,
	fleet
}) => {
	await installed(page, fleet.base);
	await deliver(page, fleet.base, SENT.replaceAll('https://phoebe.example.com', fleet.base));
	await expect
		.poll(() => shown(page))
		.toEqual([
			{
				title: 'lavalink failed on oracle-1',
				body: 'UnitFailed, firing',
				tag: SENT_ID,
				data: { navigate: `/alerts/${SENT_ID}` }
			}
		]);
});

test('a push it cannot read still shows a notification, so the permission stays', async ({
	page,
	fleet
}) => {
	await installed(page, fleet.base);
	await deliver(page, fleet.base, 'not a declarative message');
	await expect
		.poll(() => shown(page))
		.toEqual([{ title: 'phoebe', body: '', tag: '', data: { navigate: '/' } }]);
});

test('a tap opens the alert in the window already open', async ({ page, fleet }) => {
	const ids = await fleet.alerts();
	await installed(page, fleet.base);
	const sent = SENT.replaceAll('https://phoebe.example.com', fleet.base).replaceAll(
		SENT_ID,
		ids.UnitFailed
	);
	await deliver(page, fleet.base, sent);
	await expect.poll(() => shown(page)).toHaveLength(1);
	const [worker] = page.context().serviceWorkers();
	// a tap is the browser's to make, so the worker hears one it did not make; the window is
	// told where to go before the handler asks the event to wait
	await worker.evaluate(async () => {
		type Click = new (type: string, init: { notification: Notification }) => Event;
		const sw = self as unknown as EventTarget & { registration: ServiceWorkerRegistration };
		const [notification] = await sw.registration.getNotifications();
		const NotificationEvent = (globalThis as unknown as { NotificationEvent: Click })
			.NotificationEvent;
		try {
			sw.dispatchEvent(new NotificationEvent('notificationclick', { notification }));
		} catch {}
	});
	await expect(page).toHaveURL(`${fleet.base}/alerts/${ids.UnitFailed}`);
	await expect(page.locator('header.hero h1')).toHaveText('lavalink.service failed');
	expect(await shown(page)).toHaveLength(0);
	expect(page.context().pages()).toHaveLength(1);
});
