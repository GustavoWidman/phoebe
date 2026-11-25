import { expect, test } from './fleet.ts';

test.use({ serviceWorkers: 'allow' });

test('the app shell opens without the network', async ({ page, context, fleet: _ }, info) => {
	test.skip(info.project.name !== 'desktop', 'Playwright drives service workers in Chromium');
	await page.goto('/');
	await page.evaluate(() => navigator.serviceWorker.ready);
	await expect(page.getByRole('heading', { level: 1 })).toBeVisible();
	await context.setOffline(true);
	await page.reload();
	await expect(page.locator('.wm')).toContainText('phoebe');
	await expect(page.getByText('Waiting for a node to answer')).toBeVisible();
	await context.setOffline(false);
	await expect(page.getByRole('heading', { level: 1 })).toHaveText('2 things need you.', {
		timeout: 20_000
	});
});
