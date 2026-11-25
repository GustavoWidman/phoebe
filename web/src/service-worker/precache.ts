/**
 * The requests a build precaches its files with. Each goes past the browser's HTTP cache
 * (`reload`) to the network: that cache may hold an older build's `index.html` it still
 * counts as fresh, and a build that cached it would serve the older app for as long as it ran.
 */
export function precacheRequests(paths: string[], origin: string): Request[] {
	return paths.map((path) => new Request(new URL(path, origin), { cache: 'reload' }));
}
