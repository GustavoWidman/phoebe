/**
 * Every request to `/api` carries this device's session and a proof over that very request. One
 * the server refuses, or a stream that says the session ended, sends the app to sign in, where
 * it waits until there is a session again; signing in returns it to the screen it left.
 */
import { router, type Route } from '#lib/router.svelte.ts';
import { device, due } from './device.ts';

/** `fetch`, signed with this device's session; a 401 means there is none the server takes */
export async function signed(url: string, init: RequestInit = {}): Promise<Response> {
	// the path and query exactly as the request line carries them, an empty `?` included
	const at = new URL(url, location.href);
	const path = at.href.slice(at.origin.length).replace(/#.*$/, '');
	const proof = await device.headers(init.method ?? 'GET', path);
	return fetch(url, {
		cache: 'no-store',
		...init,
		headers: { ...(init.headers as Record<string, string> | undefined), ...proof }
	});
}

/** where signing in returns to */
let left: Route = { screen: 'fleet' };
let waiting: Promise<void> | null = null;

/** ends the session, opens the sign-in screen, and resolves once signed in again */
export function refused(): Promise<void> {
	return (waiting ??= (async () => {
		if (router.route.screen !== 'signin') left = router.route;
		await device.end();
		router.go({ screen: 'signin' }, true);
		await device.signedIn();
		waiting = null;
	})());
}

/** the screen signing in returns to */
export const back = () => left;

/** renews a session past a day old, unseen, when a sign-in node answers; the next once shown */
export async function renew() {
	const session = await device.session();
	if (session && due(session, Date.now())) (await import('./signin.ts')).renew();
}
