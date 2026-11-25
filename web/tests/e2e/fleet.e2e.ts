import { CLEAR, expect, test } from './fleet.ts';

test('while things fire, the fleet counts them and gives each its severity band', async ({
	page,
	fleet
}) => {
	const ids = await fleet.alerts();
	await page.goto('/');
	await expect(page.getByRole('heading', { level: 1 })).toHaveText('2 things need you.');
	const page_ = page.locator(`.band[data-alert="${ids.UnitFailed}"]`);
	const info = page.locator(`.band[data-alert="${ids.CertExpiring}"]`);
	// a page alert takes the critical treatment, an info alert the warning one
	await expect(page_).toHaveClass(/\bsev\b/);
	await expect(page_).not.toHaveClass(/sev-warn/);
	await expect(page_.locator('.sk')).toContainText('Firing · page · UnitFailed');
	await expect(page_.locator('.dot.pulse')).toHaveCount(1);
	await expect(info).toHaveClass(/sev-warn/);
	await expect(info.locator('.sk')).toContainText('Firing · info · CertExpiring');
	await expect(info.locator('.dot.pulse')).toHaveCount(0);
	await expect(page.getByText('lab can’t reach portfolio', { exact: false })).toBeVisible();
});

test('machines show their vitals, and a desktop rests rather than fails', async ({
	page,
	fleet: _
}) => {
	await page.goto('/');
	const oracle = page.locator('[data-machine="oracle-1"]').first();
	await expect(oracle).toContainText('oracle-1');
	await expect(oracle).toContainText(/1 failed|lavalink\.service failed/);
	const laptop = page.locator('[data-machine="laptop-mac"]').first();
	await expect(laptop).toContainText('resting since 13:05');
	await expect(laptop.locator('.dot')).toHaveAttribute('data-s', 'asleep');
});

test('services show which nodes reach them', async ({ page, fleet: _ }) => {
	await page.goto('/');
	const portfolio = page.locator('[data-service="portfolio"]').first();
	await expect(portfolio.locator('.dot').first()).toHaveAttribute('data-s', 'warn');
	// the phone counts the nodes that reach it, the desktop table names the one that times out
	await expect(portfolio).toContainText(/3 of 4 nodes reach it|timeout/);
});

test('all clear, the fleet is calm', async ({ page, fleet }) => {
	await fleet.seek(CLEAR);
	await page.goto('/');
	await expect(page.getByRole('heading', { level: 1 })).toHaveText('Fleet');
	await expect(page.locator('.tb .st')).toContainText(
		'All clear · 4 of 4 nodes up · 1 desktop resting'
	);
	await expect(page.locator('.band')).toHaveCount(0);
});

test('a firing alert that resolves leaves the bands live', async ({ page, fleet }) => {
	await page.goto('/');
	await expect(page.locator('.bands .band')).toHaveCount(2);
	await fleet.seek(CLEAR);
	await expect(page.locator('.bands .band')).toHaveCount(0, { timeout: 20_000 });
	await expect(page.getByRole('heading', { level: 1 })).toHaveText('Fleet');
});
