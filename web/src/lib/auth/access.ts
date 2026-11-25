/**
 * Every request to `/api` carries this device's session and a proof over that very request. One
 * the server refuses, or a stream that says the session ended, sends the app to sign in, where
 * it waits until there is a session again; signing in returns it to the screen it left.
 */
import { router, type Route } from '#lib/router.svelte.ts';
import { device } from './device.ts';

/** `fetch`, signed with this device's session; a 401 means there is none the server takes */
export async function signed(url: string, init: RequestInit = {}): Promise<Response> {
	const at = new URL(url, location.href);
	const proof = await device.headers(init.method ?? 'GET', at.pathname + at.search);
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

/** a sign-in address as its machine's name */
export const name = (via: string) => {
	try {
		return new URL(via).hostname.split('.')[0];
	} catch {
		return via;
	}
};
