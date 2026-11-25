import { expect, test } from './fleet.ts';

test('on a phone the log sheet rises from the dock', async ({ page, fleet: _ }, info) => {
	test.skip(info.project.name !== 'phone', 'the phone layout');
	await page.goto('/machines/oracle-1');
	await expect(page.locator('.dock')).toContainText('oracle-1 · live');
	await expect(page.locator('.sw')).not.toHaveClass(/open/);
	await page.locator('.dock').click();
	await expect(page.locator('.sw')).toHaveClass(/open/);
	await expect(page.locator('.sheet .logs .l').first()).toBeVisible();
	await page.getByRole('button', { name: 'Done' }).click();
	await expect(page.locator('.sw')).not.toHaveClass(/open/);
});

test('on a phone the nav bar goes back to the fleet', async ({ page, fleet: _ }, info) => {
	test.skip(info.project.name !== 'phone', 'the phone layout');
	await page.goto('/');
	await page.locator('a[data-machine="oracle-1"]').click();
	await expect(page).toHaveURL(/\/machines\/oracle-1$/);
	await page.locator('.nav .back').click();
	await expect(page).toHaveURL(/\/$/);
});

test('on a desktop the screen sits between the sidebar and the live tail', async ({
	page,
	fleet: _
}, info) => {
	test.skip(info.project.name !== 'desktop', 'the desktop layout');
	await page.goto('/');
	await expect(page.locator('.desk > aside.side')).toBeVisible();
	await expect(page.locator('.desk > main.main')).toBeVisible();
	await expect(page.locator('.tail .logs .l').first()).toBeVisible();
	await expect(page.locator('table.tbl')).toHaveCount(2);
	await page.locator('.side a', { hasText: 'oracle-1' }).click();
	await expect(page.locator('.tail-h')).toContainText('oracle-1');
	await page.locator('.side a', { hasText: 'Logs' }).click();
	await expect(page.locator('.tail')).toHaveCount(0);
});
