import { CLEAR, expect, test } from './fleet.ts';

test('a notification tap opens the alert under its severity header', async ({ page, fleet }) => {
	const ids = await fleet.alerts();
	await page.goto(`/alerts/${ids.UnitFailed}`);
	const hero = page.locator('header.hero');
	await expect(hero).toHaveClass(/\bsev\b/);
	await expect(hero.getByRole('heading', { level: 1 })).toHaveText('lavalink.service failed');
	await expect(page.locator('.lbl', { hasText: 'Seen by' })).toContainText('4 of 4 nodes');
	await expect(page.locator('.tl')).toContainText('Fires on oracle-1, then the 3 other nodes');
	await expect(page.locator('.tl')).toContainText('Notification sent by');
	await expect(page.locator('.ch .mk')).toHaveCount(1);
});

test('a resolved alert drops the severity and says how long it lasted', async ({ page, fleet }) => {
	const ids = await fleet.alerts();
	await page.goto(`/alerts/${ids.DiskAlmostFull}`);
	const hero = page.locator('header.hero');
	await expect(hero).toHaveClass(/\bres\b/);
	await expect(hero).not.toHaveClass(/\bsev\b/);
	await expect(hero.locator('.sk')).toContainText('Resolved · page · DiskAlmostFull');
	await expect(hero).toContainText('lasted');
	await expect(page.locator('.tl')).toContainText('Resolved on all 4 nodes');
});

test('an alert that resolves while open loses its severity', async ({ page, fleet }) => {
	const ids = await fleet.alerts();
	await page.goto(`/alerts/${ids.UnitFailed}`);
	await expect(page.locator('header.hero')).toHaveClass(/\bsev\b/);
	await fleet.seek(CLEAR);
	await expect(page.locator('header.hero')).toHaveClass(/\bres\b/, { timeout: 20_000 });
});

test('the machine charts its metrics over the range chosen', async ({ page, fleet: _ }) => {
	await page.goto('/machines/oracle-1');
	await expect(page.getByRole('heading', { level: 1 })).toHaveText('oracle-1');
	for (const name of ['CPU', 'Memory', 'Disk /', 'Network', 'Load'])
		await expect(page.locator(`[data-chart="${name}"] .ch svg`)).toBeVisible();
	await expect(page.locator('.lbl', { hasText: 'Units' })).toContainText('1 failed');
	const ticks = page.locator('[data-chart="CPU"] .ch-t');
	const before = await ticks.textContent();
	const answered = page.waitForResponse(
		(r) => r.url().includes('metric=cpu') && r.url().includes('range=24h')
	);
	await page.getByRole('button', { name: '24h', exact: true }).click();
	await answered;
	await expect(ticks).not.toHaveText(before!);
});

test('tapping a node on the probe map shows its latency', async ({ page, fleet: _ }) => {
	await page.goto('/machines/oracle-2/services/portfolio');
	// the node that cannot reach it comes selected
	await expect(page.locator('.lbl', { hasText: 'Latency from lab' })).toContainText(
		'timeout since'
	);
	await expect(page.locator('.map g[data-node="lab"]')).toHaveClass(/\bx\b/);
	await page.locator('.map g[data-node="oracle-2"]').click();
	await expect(page.locator('.lbl', { hasText: 'Latency from oracle-2' })).toContainText(/ms now/);
	await expect(page.locator('.map g[data-node="oracle-2"]')).toHaveClass(/\bsel\b/);
});

test('logs search highlights what matches and the tail brings new lines', async ({
	page,
	fleet
}) => {
	await page.goto('/logs');
	await page.getByRole('searchbox').fill('lavalink');
	await expect(page.locator('.logs mark').first()).toHaveText('lavalink', { ignoreCase: true });
	await page.getByRole('searchbox').fill('');
	await expect(page.locator('.logs .l').first()).toBeVisible();
	await fleet.seek(0, 20);
	await expect(page.locator('.logs .l.new').first()).toBeAttached({ timeout: 20_000 });
});

test('the device screen holds the theme', async ({ page, fleet: _ }) => {
	await page.goto('/device');
	await page.getByRole('button', { name: 'dark', exact: true }).click();
	await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
	await page.reload();
	await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
	await page.getByRole('button', { name: 'auto', exact: true }).click();
	await expect(page.locator('html')).not.toHaveAttribute('data-theme', /./);
});
