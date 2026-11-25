import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, test } from 'vitest';
import { canon, checkCss, checkMarkup, readSystem } from './design-gate.ts';

const sys = readSystem(readFileSync(resolve(import.meta.dirname, '../design/system.md'), 'utf8'));
const block = (selector: string, tokens: Record<string, string>, scheme: string) =>
	`${selector}{${Object.entries(tokens)
		.map(([k, v]) => `${k}:${v}`)
		.join(';')};color-scheme:${scheme}}`;
const themed = [
	block(':root', sys.light, 'light'),
	`@media (prefers-color-scheme:dark){${block(':root:not([data-theme=light])', sys.dark, 'dark')}}`,
	block(':root[data-theme=dark]', sys.dark, 'dark'),
	'body{background:var(--bg)}'
].join('');

test('a minifier’s spelling of a token is the token', () => {
	expect(canon('rgba(20, 20, 19, 0.1)')).toBe(canon('#1414131a'));
	expect(canon('oklch(0.54 0.2 295)')).toBe(canon('oklch(54% .2 295)'));
	expect(canon('color-mix(in oklab, var(--acc) 11%, transparent)')).toBe(
		canon('color-mix(in oklab,var(--acc) 11%,transparent)')
	);
});

describe('the built CSS', () => {
	test('passes when every value is the system’s', () => {
		const css =
			themed +
			'.row{padding:12px 0;border-bottom:1px solid var(--rule);font:600 15px/1.4 var(--f)}';
		expect(checkCss({ 'app.css': css }, sys).errs).toEqual([]);
	});

	test('names every value off the system', () => {
		const css =
			themed +
			'.x{color:#fff;padding:5px;font-size:14px;border-radius:4px;border-top:2.5px solid var(--rule);font-weight:700;background:var(--brand)}';
		expect(checkCss({ 'app.css': css }, sys).errs).toEqual([
			'app.css: .x: color: raw color #fff',
			'app.css: .x: padding 5px is off the spacing scale',
			'app.css: .x: font-size 14px is off the scale',
			'app.css: .x: radius 4px is not 0, 8px, 999px',
			'app.css: .x: border-top 2.5px is not a rule weight',
			'app.css: .x: font-weight 700',
			'app.css: .x: background: var(--brand) is neither a token nor a local property'
		]);
	});

	test('holds the token blocks to system.md', () => {
		const css = themed.replace('--acc:oklch(0.54 0.2 295)', '--acc:oklch(0.55 0.2 262)');
		expect(checkCss({ 'app.css': css }, sys).errs).toEqual([
			':root (light): --acc is "oklch(0.55 0.2 262)", system.md says "oklch(0.54 0.2 295)"'
		]);
	});
});

test('the markup may not carry its own colors, radii or weights', () => {
	const svelte =
		'<script>let a = 1;</script>\n<svg><rect rx="3" fill="#123456" stroke-width="2.5" /></svg>';
	expect(checkMarkup('X.svelte', svelte, sys, new Set())).toEqual([
		'X.svelte:2: svg rx=3; use the radius scale in CSS',
		'X.svelte:2: fill="#123456"; color comes from CSS',
		'X.svelte:2: stroke-width 2.5 is not a rule weight',
		'X.svelte:2: raw color #123456'
	]);
});
