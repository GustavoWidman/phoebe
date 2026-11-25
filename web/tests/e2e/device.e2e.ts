import type { Page } from '@playwright/test';
import { expect, test } from './fleet.ts';
import { stubPush, type PushStub } from './push.ts';

// signed in, so the entry the device screen shows carries the session's device name
test.use({ signin: true });

/** the key the simulated fleet serves, and the one subscription in its list */
const KEY =
	'BBgqG0VbQAPnBlmSzWD_uRkfTJJ7rNkfZc3HbxlUfhd8DBhqYt_DqIxDFK71gxU01Umj-1CqpRCOP0NQP_4183g';
const LISTED = 'https://web.push.apple.com/QGxfixture-iphone';
const NEW = 'https://web.push.apple.com/QGxnew-iphone';

/** signed in, then on the device screen with the browser's push as `stub` says */
async function open(page: Page, stub: PushStub) {
	await page.addInitScript(stubPush, stub);
	await page.goto('/signin');
	await expect(page.locator('.auth h1')).toHaveText('Signed in on the tailnet');
	await page.goto('/device');
	return page.locator('section.push');
}

test('the device screen shows the session it reads as', async ({ page, fleet: _ }) => {
	await open(page, { permission: 'default', subscription: null, subscribe: { endpoint: NEW } });
	await expect(page.locator('[data-session]')).toContainText('gustavo@github');
	await expect(page.locator('[data-session]')).toContainText('iphone');
	await expect(page.locator('.lbl', { hasText: 'Session' })).toContainText('until');
});

test('turning notifications on shows the entry to add to the list, and how', async ({
	page,
	fleet: _
}) => {
	const push = await open(page, {
		permission: 'default',
		subscription: null,
		subscribe: { endpoint: NEW }
	});
	await expect(push).toHaveAttribute('data-push', 'off');
	await expect(push.locator('.pr b')).toHaveText('Off');
	await push.getByRole('button', { name: 'Turn on' }).click();
	await expect(push).toHaveAttribute('data-push', 'uncommitted');
	await expect(push.locator('.pr b')).toHaveText('Subscribed · not in the list yet');
	const entry = JSON.parse((await push.locator('.entry pre').textContent())!);
	expect(entry).toMatchObject({ name: 'iphone', endpoint: NEW });
	expect(Object.keys(entry.keys).sort()).toEqual(['auth', 'p256dh']);
	await expect(push.locator('.cmd')).toContainText(
		'agenix -e secrets/phoebe-push-subscriptions.age # add this entry to the array'
	);

	await push.getByRole('button', { name: 'Turn off' }).click();
	await expect(push).toHaveAttribute('data-push', 'off');
	await expect(push.locator('.entry')).toHaveCount(0);
});

test('on a phone, the entry and its command scroll into view above the log dock', async ({
	page,
	fleet: _
}, info) => {
	test.skip(info.project.name !== 'phone', 'the dock is the phone frame');
	const push = await open(page, {
		permission: 'default',
		subscription: null,
		subscribe: { endpoint: NEW }
	});
	await push.getByRole('button', { name: 'Turn on' }).click();
	const cmd = push.locator('.cmd');
	await cmd.evaluate((el) => el.scrollIntoView({ block: 'end' }));
	const bottom = await cmd.evaluate((el) => el.getBoundingClientRect().bottom);
	const dock = await page.locator('.dock').evaluate((el) => el.getBoundingClientRect().top);
	expect(bottom).toBeLessThanOrEqual(dock);
});

test('a device the list already holds is on, and turning it off names the entry to remove', async ({
	page,
	fleet: _
}) => {
	const push = await open(page, {
		permission: 'granted',
		subscription: { endpoint: LISTED, key: KEY },
		subscribe: { endpoint: NEW }
	});
	await expect(push).toHaveAttribute('data-push', 'on');
	await expect(push.locator('.pr b')).toHaveText('On');
	await expect(push.locator('.pr .dot')).toHaveAttribute('data-s', 'ok');
	await push.getByRole('button', { name: 'Turn off' }).click();
	await expect(push.locator('.note')).toContainText('The list still has "iphone"');
});

test('notifications blocked in the browser say where to allow them', async ({ page, fleet: _ }) => {
	const push = await open(page, {
		permission: 'denied',
		subscription: null,
		subscribe: { endpoint: NEW }
	});
	await expect(push).toHaveAttribute('data-push', 'denied');
	await expect(push.locator('.pr b')).toHaveText('Off in site settings');
	await expect(push.getByRole('button')).toHaveCount(0);
});

test("an iPhone's Safari tab is sent to the Home Screen app", async ({ page, fleet: _ }) => {
	const push = await open(page, {
		supported: false,
		ios: true,
		permission: 'default',
		subscription: null,
		subscribe: { endpoint: NEW }
	});
	await expect(push).toHaveAttribute('data-push', 'install');
	await expect(push.locator('.pr b')).toHaveText(
		'Add phoebe to the Home Screen to get notifications'
	);
	await expect(push.getByRole('button')).toHaveCount(0);
});

test('the Home Screen app on an iPhone subscribes', async ({ page, fleet: _ }) => {
	const push = await open(page, {
		ios: true,
		standalone: true,
		permission: 'default',
		subscription: null,
		subscribe: { endpoint: NEW }
	});
	await push.getByRole('button', { name: 'Turn on' }).click();
	await expect(push).toHaveAttribute('data-push', 'uncommitted');
});
