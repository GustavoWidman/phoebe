/**
 * The app's screens as URLs, kept in the browser's history. A notification for an alert opens
 * `/alerts/<id>`, so every screen deep-links. A path that names no screen becomes `/`.
 */
export type Route =
	| { screen: 'fleet' }
	| { screen: 'machine'; name: string }
	| { screen: 'service'; name: string }
	| { screen: 'alert'; id: string }
	| { screen: 'logs' }
	/** this device: its theme now, and its session and notifications once the app signs in */
	| { screen: 'device' };

const routes: [RegExp, (...params: string[]) => Route][] = [
	[/^\/$/, () => ({ screen: 'fleet' })],
	[/^\/machines\/([^/]+)$/, (name) => ({ screen: 'machine', name })],
	[/^\/services\/([^/]+)$/, (name) => ({ screen: 'service', name })],
	[/^\/alerts\/([^/]+)$/, (id) => ({ screen: 'alert', id })],
	[/^\/logs$/, () => ({ screen: 'logs' })],
	[/^\/device$/, () => ({ screen: 'device' })]
];

export function parse(path: string): Route | null {
	path = path.replace(/(.)\/$/, '$1');
	for (const [pattern, route] of routes) {
		const m = pattern.exec(path);
		if (!m) continue;
		try {
			return route(...m.slice(1).map(decodeURIComponent));
		} catch {
			return null;
		}
	}
	return null;
}

export function href(route: Route): string {
	const e = encodeURIComponent;
	switch (route.screen) {
		case 'fleet':
			return '/';
		case 'machine':
			return `/machines/${e(route.name)}`;
		case 'service':
			return `/services/${e(route.name)}`;
		case 'alert':
			return `/alerts/${e(route.id)}`;
		case 'logs':
			return '/logs';
		case 'device':
			return '/device';
	}
}

type Entry = { depth: number };
const depthOf = (state: unknown) => (state as Entry | null)?.depth ?? 0;

function open() {
	const depth = depthOf(history.state);
	const route = parse(location.pathname);
	history.replaceState({ depth } satisfies Entry, '', route ? undefined : '/');
	return { route: route ?? ({ screen: 'fleet' } as Route), depth };
}

let nav = $state.raw(open());

function go(route: Route, replace = false) {
	const path = href(route);
	if (path === location.pathname) return;
	const depth = nav.depth + (replace ? 0 : 1);
	if (replace) history.replaceState({ depth } satisfies Entry, '', path);
	else history.pushState({ depth } satisfies Entry, '', path);
	nav = { route, depth };
}

addEventListener('popstate', (event) => {
	nav = { route: parse(location.pathname) ?? { screen: 'fleet' }, depth: depthOf(event.state) };
});

// A plain click on a link to one of the app's screens navigates without reloading.
addEventListener('click', (event) => {
	if (event.defaultPrevented || event.button !== 0) return;
	if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
	const a = event.target instanceof Element && event.target.closest('a[href]');
	if (!(a instanceof HTMLAnchorElement) || a.target || a.hasAttribute('download')) return;
	const route = a.origin === location.origin && parse(a.pathname);
	if (!route) return;
	event.preventDefault();
	go(route);
});

export const router = {
	get route() {
		return nav.route;
	},
	go,
	/** back to where the app came from, or to `route` when it opened here */
	back: (route: Route) => (nav.depth > 0 ? history.back() : go(route, true))
};
