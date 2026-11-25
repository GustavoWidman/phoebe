/**
 * Holds the built app to design/system.md: every color, font size, font family, font weight,
 * radius, spacing value and rule weight the build ships must be one the system lists. It reads
 * the built CSS, as the minifier wrote it, and the markup of every component, where inline
 * styles and SVG attributes live. Exits 1 on any stray value, naming it.
 *
 *   bun scripts/design-gate.ts [build dir]          (default: build/, after `bun run build`)
 */
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';
import postcss, { type AtRule, type Declaration, type Node, type Rule } from 'postcss';

const WEB = resolve(import.meta.dirname, '..');

/* ---------- the system ---------- */

export type System = {
	light: Record<string, string>;
	dark: Record<string, string>;
	sizes: Set<string>;
	families: Set<string>;
	spaces: Set<string>;
	radii: Set<string>;
	rules: Set<number>;
};

export function readSystem(spec: string): System {
	const fence = (name: string) => {
		const m = spec.match(new RegExp('```' + name + '\\n([\\s\\S]*?)```'));
		if (!m) throw new Error(`system.md has no \`\`\`${name} block`);
		return m[1].trim().split('\n');
	};
	const pairs = (lines: string[]) =>
		Object.fromEntries(
			lines.map((l) => {
				const i = l.indexOf(':');
				return [l.slice(0, i).trim(), l.slice(i + 1).trim()];
			})
		);
	const scale = Object.fromEntries(
		Object.entries(pairs(fence('scale'))).map(([k, v]) => [k, v.split(/\s+(?![^(]*\))/)])
	);
	return {
		light: pairs(fence('tokens')),
		dark: pairs(fence('tokens-dark')),
		sizes: new Set(scale['font-size']),
		families: new Set(scale.family),
		spaces: new Set(scale.space),
		radii: new Set(scale.radius),
		rules: new Set(scale.rule.map(parseFloat))
	};
}

/** the two font stacks, and the one easing curve, that live beside the tokens */
const FONT_VARS: Record<string, RegExp> = {
	'--f': /^["']?Instrument Sans["']?,/,
	'--m': /^["']?Geist Mono["']?,/
};
const OTHER_VARS = new Set(['--e']);
/** the families the stacks load as web fonts */
const FACES = new Set(['Instrument Sans', 'Geist Mono']);
const WEIGHTS = new Set(['400', '500', '600']);

/* ---------- values, as a minifier may rewrite them ---------- */

const hex2 = (n: number) =>
	Math.round(Math.max(0, Math.min(255, n)))
		.toString(16)
		.padStart(2, '0');

/**
 * A value in one spelling: `rgba(20, 20, 19, 0.1)` and `#1414131a` are one color,
 * `oklch(0.54 0.2 295)` and `oklch(54% .2 295)` another, and `0.5` is `.5`.
 */
export function canon(value: string): string {
	return value
		.toLowerCase()
		.replace(/["']/g, '')
		.replace(
			/rgba?\(\s*([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)(?:[\s,/]+([\d.]+%?))?\s*\)/g,
			(_, r, g, b, a) => {
				const alpha = a === undefined ? 1 : a.endsWith('%') ? parseFloat(a) / 100 : parseFloat(a);
				return `#${hex2(+r)}${hex2(+g)}${hex2(+b)}${hex2(alpha * 255)}`;
			}
		)
		.replace(/#([0-9a-f]{3,8})\b/g, (_, h: string) => {
			const full = h.length <= 4 ? [...h].map((c) => c + c).join('') : h;
			return `#${full.length === 6 ? full + 'ff' : full}`;
		})
		.replace(
			/oklch\(\s*([\d.]+)(%?)/g,
			(_, l, pct) => `oklch(${pct ? parseFloat(l) / 100 : parseFloat(l)}`
		)
		.replace(/-?\d*\.?\d+/g, (n) => String(parseFloat(n)))
		.replace(/\s*([(),/])\s*/g, '$1')
		.replace(/\s+/g, ' ')
		.trim();
}

/* ---------- what each check reads ---------- */

const NAMED = new Set(
	'aliceblue antiquewhite aqua aquamarine azure beige bisque black blanchedalmond blue blueviolet brown burlywood cadetblue chartreuse chocolate coral cornflowerblue cornsilk crimson cyan darkblue darkcyan darkgoldenrod darkgray darkgreen darkgrey darkkhaki darkmagenta darkolivegreen darkorange darkorchid darkred darksalmon darkseagreen darkslateblue darkslategray darkturquoise darkviolet deeppink deepskyblue dimgray dodgerblue firebrick floralwhite forestgreen fuchsia gainsboro ghostwhite gold goldenrod gray green greenyellow grey honeydew hotpink indianred indigo ivory khaki lavender lawngreen lemonchiffon lightblue lightcoral lightcyan lightgray lightgreen lightgrey lightpink lightsalmon lightseagreen lightskyblue lightslategray lightsteelblue lightyellow lime limegreen linen magenta maroon mediumblue mediumorchid mediumpurple mediumseagreen mediumslateblue mediumspringgreen mediumturquoise mintcream mistyrose moccasin navajowhite navy oldlace olive olivedrab orange orangered orchid palegreen paleturquoise palevioletred papayawhip peachpuff peru pink plum powderblue purple rebeccapurple red rosybrown royalblue saddlebrown salmon sandybrown seagreen seashell sienna silver skyblue slateblue slategray snow springgreen steelblue tan teal thistle tomato turquoise violet wheat white whitesmoke yellow yellowgreen'.split(
		' '
	)
);
const COLOR_PROP =
	/^(?:background(?:-color|-image)?|color|fill|stroke|border(?:-(?:top|right|bottom|left))?(?:-color)?|outline(?:-color)?|box-shadow|text-decoration(?:-color)?|caret-color|accent-color|stop-color|column-rule(?:-color)?)$/;
const COLOR_FN = /\b(rgba?|hsla?|hwb|lab|lch|oklab|oklch|color-mix|color)\(/;
const HEX = /#(?:[0-9a-f]{8}|[0-9a-f]{6}|[0-9a-f]{3,4})\b/i;
const SPACE_PROP =
	/^(?:(?:padding|margin)(?:-(?:top|right|bottom|left|inline|block)(?:-(?:start|end))?)?|gap|row-gap|column-gap)$/;
const RADIUS_PROP = /^border(?:-(?:top|bottom)-(?:left|right))?-radius$/;
const RULE_PROP = /^border(?:-(?:top|right|bottom|left))?(?:-width)?$/;

/** the problems with one declaration outside the token blocks; `locals` are the properties the CSS defines */
export function checkDecl(prop: string, value: string, sys: System, locals: Set<string>): string[] {
	const errs: string[] = [];
	// a minifier writes `transparent` as #0000
	const v = value.trim().replace(/#0000(?:0000)?\b/gi, 'transparent');
	const bare = v.replace(/var\([^)]*\)/g, ' ');
	if (HEX.test(bare)) errs.push(`${prop}: raw color ${bare.match(HEX)![0]}`);
	if (COLOR_FN.test(bare))
		errs.push(`${prop}: color function ${bare.match(COLOR_FN)![1]}() outside the token blocks`);
	if (COLOR_PROP.test(prop))
		for (const w of bare.toLowerCase().split(/[^a-z]+/))
			if (NAMED.has(w)) errs.push(`${prop}: named color "${w}"`);
	for (const m of v.matchAll(/var\((--[\w-]+)/g)) {
		const n = m[1];
		if (!(n in sys.light) && !FONT_VARS[n] && !OTHER_VARS.has(n) && !locals.has(n))
			errs.push(`${prop}: var(${n}) is neither a token nor a local property`);
	}
	if (prop === 'font-size') {
		if (!sys.sizes.has(v)) errs.push(`font-size ${v} is off the scale`);
	} else if (prop === 'font') {
		if (v !== 'inherit') {
			const sz = v.match(/(\d*\.?\d+px)(?:\/[\d.]+)?/);
			if (!sz) errs.push(`font shorthand without a size: "${v}"`);
			else {
				if (!sys.sizes.has(sz[1])) errs.push(`font size ${sz[1]} is off the scale`);
				const fam = v.slice(v.indexOf(sz[0]) + sz[0].length).trim();
				if (!sys.families.has(fam))
					errs.push(`font family "${fam}" is not ${[...sys.families].join(' or ')}`);
				const w = v.match(/^(\d{3})\s/);
				if (w && !WEIGHTS.has(w[1])) errs.push(`font-weight ${w[1]}`);
			}
		}
	} else if (prop === 'font-family') {
		if (!sys.families.has(v) && v !== 'inherit')
			errs.push(`font-family "${v}" is not a system family`);
	} else if (prop === 'font-weight') {
		if (!WEIGHTS.has(v) && v !== 'inherit') errs.push(`font-weight ${v}`);
	} else if (RADIUS_PROP.test(prop)) {
		for (const r of v.split(/\s+/))
			if (!sys.radii.has(r)) errs.push(`radius ${r} is not ${[...sys.radii].join(', ')}`);
	} else if (SPACE_PROP.test(prop)) {
		const calc = /calc\(/.test(v);
		for (const n of v
			.replace(/var\([^)]*\)|env\([^)]*\)/g, ' ')
			.matchAll(/-?\d*\.?\d+([a-z%]+)?/g)) {
			const raw = n[0].replace(/^-/, '');
			if (parseFloat(raw) === 0) continue;
			if (!n[1]) {
				if (!calc) errs.push(`${prop} unitless ${n[0]}`);
			} else if (n[1] !== 'px') errs.push(`${prop} uses ${n[0]}; spacing is px from the scale`);
			else if (!sys.spaces.has(raw)) errs.push(`${prop} ${n[0]} is off the spacing scale`);
		}
	} else if (RULE_PROP.test(prop)) {
		for (const n of v.matchAll(/(\d*\.?\d+)px/g))
			if (!sys.rules.has(parseFloat(n[1]))) errs.push(`${prop} ${n[0]} is not a rule weight`);
	} else if (prop === 'stroke-width') {
		if (!sys.rules.has(parseFloat(v))) errs.push(`stroke-width ${v} is not a rule weight`);
	}
	return errs;
}

/* ---------- the built CSS ---------- */

const isRule = (n: Node | undefined): n is Rule => n?.type === 'rule';
const inMedia = (n: Node, re: RegExp) => {
	for (let p = n.parent as Node | undefined; p; p = p.parent as Node | undefined)
		if (p.type === 'atrule' && (p as AtRule).name === 'media' && re.test((p as AtRule).params))
			return true;
	return false;
};
const inFace = (n: Node) =>
	n.parent?.type === 'atrule' && (n.parent as AtRule).name === 'font-face';
const sel = (s: string) => s.replace(/["'\s]/g, '');

/** checks every stylesheet in `css`, keyed by its name; returns the problems and what it counted */
export function checkCss(css: Record<string, string>, sys: System, markupLocals: string[] = []) {
	const errs: string[] = [];
	const counts = { tokens: 0, declarations: 0 };
	const blocks = {
		light: [] as Rule[],
		darkMedia: [] as Rule[],
		darkAttr: [] as Rule[]
	};
	const roots = Object.entries(css).map(
		([name, text]) => [name, postcss.parse(text, { from: name })] as const
	);
	const locals = new Set(markupLocals);
	for (const [, root] of roots) root.walkDecls(/^--/, (d) => void locals.add(d.prop));

	for (const [name, root] of roots) {
		root.walkRules((r) => {
			const s = sel(r.selector);
			if (s === ':root' && !inMedia(r, /./)) blocks.light.push(r);
			else if (s === ':root:not([data-theme=light])' && inMedia(r, /prefers-color-scheme:\s*dark/))
				blocks.darkMedia.push(r);
			else if (s === ':root[data-theme=dark]' && !inMedia(r, /./)) blocks.darkAttr.push(r);
		});
		const tokenRules = new Set<Node>([...blocks.light, ...blocks.darkMedia, ...blocks.darkAttr]);
		root.walkDecls((d: Declaration) => {
			if (tokenRules.has(d.parent as Node)) return;
			const at = `${name}: ${isRule(d.parent) ? d.parent.selector : '@' + (d.parent as AtRule).name}`;
			if (inFace(d)) {
				if (d.prop === 'font-family' && !FACES.has(d.value.replace(/["']/g, '').trim()))
					errs.push(`${at}: @font-face loads "${d.value}", not a system family`);
				return;
			}
			counts.declarations++;
			for (const e of checkDecl(d.prop, d.value, sys, locals)) errs.push(`${at}: ${e}`);
		});
		root.walkAtRules(
			'import',
			(a) => void errs.push(`${name}: @import ${a.params} loads a stylesheet from elsewhere`)
		);
	}

	const wantBlock = (rules: Rule[], want: Record<string, string>, label: string, extra = false) => {
		if (rules.length !== 1) return void errs.push(`${label}: ${rules.length} blocks, want 1`);
		const got: Record<string, string> = {};
		rules[0].walkDecls((d) => void (got[d.prop] = d.value));
		for (const [k, v] of Object.entries(want)) {
			if (!(k in got)) errs.push(`${label}: missing ${k}`);
			else if (canon(got[k]) !== canon(v))
				errs.push(`${label}: ${k} is "${got[k]}", system.md says "${v}"`);
			else counts.tokens++;
		}
		for (const [k, v] of Object.entries(got)) {
			if (k in want || k === 'color-scheme') continue;
			if (extra && FONT_VARS[k]) {
				if (!FONT_VARS[k].test(v))
					errs.push(`${label}: ${k} must start with the system family, got "${v}"`);
				continue;
			}
			if (extra && OTHER_VARS.has(k)) continue;
			errs.push(`${label}: ${k} is not a system.md token`);
		}
		if (!/^(light|dark)$/.test(got['color-scheme'] ?? '')) errs.push(`${label}: no color-scheme`);
	};
	wantBlock(blocks.light, sys.light, ':root (light)', true);
	wantBlock(blocks.darkMedia, sys.dark, '@media dark, guarded by :root:not([data-theme="light"])');
	wantBlock(blocks.darkAttr, sys.dark, ':root[data-theme="dark"]');

	let background = false;
	for (const [, root] of roots)
		root.walkRules((r) => {
			if (r.selector.split(',').some((s) => s.trim() === 'body'))
				r.walkDecls(
					/^background(-color)?$/,
					(d) => void (background ||= d.value.trim() === 'var(--bg)')
				);
		});
	if (!background) errs.push('body has no explicit background of var(--bg)');
	return { errs, counts };
}

/* ---------- the markup ---------- */

/**
 * The markup of a component: what is left once its script and style blocks are blanked, so
 * line numbers hold. Inline styles and SVG presentation attributes are checked as declarations.
 */
export function checkMarkup(
	name: string,
	source: string,
	sys: System,
	locals: Set<string>
): string[] {
	const errs: string[] = [];
	const markup = source.replace(/<(script|style)\b[\s\S]*?<\/\1>/g, (b) =>
		b.replace(/[^\n]/g, ' ')
	);
	const line = (i: number) => markup.slice(0, i).split('\n').length;
	const attrs =
		/(?:\s|^)(style:([\w-]+)|style|fill|stroke|stroke-width|rx|ry|font-size|font-family|font-weight|color)=(?:"([^"]*)"|\{([^}]*)\}|'([^']*)')/g;
	for (const m of markup.matchAll(attrs)) {
		const [, attr, styleProp] = m;
		const value = m[3] ?? m[5] ?? `{${m[4]}}`;
		const at = `${name}:${line(m.index)}`;
		const check = (prop: string, v: string) => {
			if (/\{/.test(v) && !prop.startsWith('--') && !/^(left|top|width|height)$/.test(prop))
				errs.push(`${at}: ${prop} computed at runtime (${v}), unverifiable`);
			else for (const e of checkDecl(prop, v, sys, locals)) errs.push(`${at}: ${e}`);
		};
		if (attr === 'style') {
			for (const part of value.split(';')) {
				const i = part.indexOf(':');
				if (i > 0) check(part.slice(0, i).trim(), part.slice(i + 1).trim());
			}
		} else if (styleProp) check(styleProp, value);
		else if (attr === 'rx' || attr === 'ry')
			errs.push(`${at}: svg ${attr}=${value}; use the radius scale in CSS`);
		else if (attr === 'fill' || attr === 'stroke') {
			if (!/^(none|currentColor|transparent)$/.test(value))
				errs.push(`${at}: ${attr}="${value}"; color comes from CSS`);
		} else check(attr, value);
	}
	for (const m of markup.matchAll(new RegExp(HEX, 'gi')))
		if (!/[\w&]/.test(markup[m.index - 1] ?? ''))
			errs.push(`${name}:${line(m.index)}: raw color ${m[0]}`);
	for (const m of markup.matchAll(new RegExp(COLOR_FN, 'g')))
		errs.push(`${name}:${line(m.index)}: color function ${m[1]}()`);
	return errs;
}

/* ---------- the run ---------- */

function* walk(dir: string, ext: RegExp): Generator<string> {
	for (const n of readdirSync(dir)) {
		const p = join(dir, n);
		if (statSync(p).isDirectory()) yield* walk(p, ext);
		else if (ext.test(p)) yield p;
	}
}

if (import.meta.main) {
	const build = resolve(process.argv[2] ?? join(WEB, 'build'));
	const sys = readSystem(readFileSync(join(WEB, 'design/system.md'), 'utf8'));
	const css = Object.fromEntries(
		[...walk(build, /\.css$/)].map((p) => [relative(build, p), readFileSync(p, 'utf8')])
	);
	if (!Object.keys(css).length)
		throw new Error(`${build} holds no stylesheet; build the app first`);
	const components = Object.fromEntries(
		[...walk(join(WEB, 'src'), /\.svelte$/)].map((p) => [relative(WEB, p), readFileSync(p, 'utf8')])
	);
	// a component may set a local property inline, as a gauge sets its fill
	const inline = Object.values(components).flatMap((c) =>
		[...c.matchAll(/style:(--[\w-]+)|[";]\s*(--[\w-]+)\s*:/g)].map((m) => m[1] ?? m[2])
	);
	const { errs, counts } = checkCss(css, sys, inline);
	const locals = new Set(inline);
	for (const text of Object.values(css))
		postcss.parse(text).walkDecls(/^--/, (d) => void locals.add(d.prop));
	for (const [name, source] of Object.entries(components))
		errs.push(...checkMarkup(name, source, sys, locals));
	const index = readFileSync(join(build, 'index.html'), 'utf8');
	for (const m of index.matchAll(/<link[^>]*rel="stylesheet"[^>]*href="([^"]+)"/g))
		if (/^(https?:)?\/\//.test(m[1]))
			errs.push(`index.html loads a stylesheet from elsewhere: ${m[1]}`);

	console.log(
		`system.md: ${Object.keys(sys.light).length} color tokens, ${sys.sizes.size} sizes, ${sys.families.size} families, ${sys.spaces.size} spaces, ${sys.radii.size} radii, ${sys.rules.size} rule weights`
	);
	console.log(
		`checked: ${Object.keys(css).length} stylesheets, ${counts.tokens} token definitions, ${counts.declarations} declarations, ${Object.keys(components).length} components`
	);
	if (errs.length) {
		console.log(`\n${errs.length} values outside the system:`);
		for (const e of errs) console.log(`  ${e}`);
		process.exit(1);
	}
	console.log('pass: every value is in design/system.md');
}
