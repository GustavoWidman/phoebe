/**
 * Keeps phoebe opening without the network. Each build precaches itself under its own cache and
 * serves every navigation from the cached shell, so a launch never waits on a node; `/api` is
 * left to the network. A new build installs in the background and takes over at the next launch.
 */
/// <reference lib="webworker" />

const sw = self as unknown as ServiceWorkerGlobalScope;

// the build writes these in once it knows every file it made
const BUILD = '__PHOEBE_BUILD__';
const FILES: string[] = JSON.parse('__PHOEBE_FILES__');

const CACHE = `phoebe-${BUILD}`;
const SHELL = '/';
const PRECACHE = [SHELL, ...FILES];
const precached = new Set(PRECACHE);

sw.addEventListener('install', (event) => {
	event.waitUntil(caches.open(CACHE).then((cache) => cache.addAll(PRECACHE)));
});

sw.addEventListener('activate', (event) => {
	const old = (key: string) => key.startsWith('phoebe-') && key !== CACHE;
	event.waitUntil(
		caches.keys().then((keys) => Promise.all(keys.filter(old).map((k) => caches.delete(k))))
	);
});

sw.addEventListener('fetch', (event) => {
	const { request } = event;
	if (request.method !== 'GET') return;
	const url = new URL(request.url);
	if (url.origin !== sw.location.origin || url.pathname.startsWith('/api/')) return;
	if (request.mode === 'navigate') event.respondWith(fromCache(SHELL, request));
	else if (precached.has(url.pathname)) event.respondWith(fromCache(url.pathname, request));
});

async function fromCache(key: string, request: Request): Promise<Response> {
	const cache = await caches.open(CACHE);
	return (await cache.match(key)) ?? fetch(request);
}
