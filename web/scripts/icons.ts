/**
 * Draws the phoebe mark and renders every icon file from it.
 *
 *   bun scripts/icons.ts            (from web/)
 *
 * The icon is the crescent of the wordmark's glyph (`moon` in src/lib/ui/Icon.svelte) on its
 * own: a violet crescent on a paper tile. coeus draws a paper ring on an ink tile, so the two
 * sit apart on a Home Screen. Rasterizing goes through
 * Chromium; set CHROMIUM to a browser binary when the Playwright-managed one is not installed.
 */
import { mkdir, writeFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { chromium } from '@playwright/test';

const root = resolve(import.meta.dirname, '..');
/** --bg and --acc of the light theme, from design/system.md */
const BG = '#f6f6f3';
const ACC = 'oklch(0.54 0.2 295)';

/**
 * The tile, with the crescent's disc `size` of it across: the disc in the accent, and the
 * paper's disc over it, moved up and to the right, which leaves the crescent.
 */
function icon({ corner, size }: { corner: number; size: number }) {
	const r = (512 * size) / 2;
	const n = (v: number) => +v.toFixed(2);
	return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512"><rect width="512" height="512" rx="${corner}" fill="${BG}"/><circle cx="${n(256 + r * 0.1)}" cy="${n(256 - r * 0.06)}" r="${n(r)}" fill="${ACC}"/><circle cx="${n(256 + r * 0.56)}" cy="${n(256 - r * 0.28)}" r="${n(r * 0.86)}" fill="${BG}"/></svg>\n`;
}

const ROUNDED = 112;
const svgs = {
	'static/favicon.svg': icon({ corner: ROUNDED, size: 0.74 })
};
const pngs: Record<string, { px: number; svg: string }> = {
	'static/icons/icon-192.png': { px: 192, svg: icon({ corner: ROUNDED, size: 0.6 }) },
	'static/icons/icon-512.png': { px: 512, svg: icon({ corner: ROUNDED, size: 0.6 }) },
	// Maskable: full bleed, the mark inside the central 80% circle that every mask keeps.
	'static/icons/icon-maskable-512.png': { px: 512, svg: icon({ corner: 0, size: 0.52 }) },
	// iOS rounds the corners itself and fills transparency with black.
	'static/apple-touch-icon.png': { px: 180, svg: icon({ corner: 0, size: 0.6 }) }
};

for (const [path, svg] of Object.entries(svgs)) {
	await writeFile(resolve(root, path), svg);
}

const browser = await chromium.launch({ executablePath: process.env.CHROMIUM || undefined });
const page = await browser.newPage({ deviceScaleFactor: 1 });
for (const [path, { px, svg }] of Object.entries(pngs)) {
	await page.setViewportSize({ width: px, height: px });
	await page.setContent(
		`<style>*{margin:0}body{background:transparent}svg{display:block;width:${px}px;height:${px}px}</style>${svg}`
	);
	const out = resolve(root, path);
	await mkdir(dirname(out), { recursive: true });
	await page.screenshot({ path: out, omitBackground: true });
	console.log(`${path} ${px}px`);
}
await browser.close();
