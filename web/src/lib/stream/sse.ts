/**
 * Server-sent events read through `fetch` rather than `EventSource`, so a request carries the
 * session like any other. A stream closes while the page is hidden, where iOS would suspend it
 * anyway, and opens again, after a growing wait, whenever it ends or fails. One the server
 * refuses, or that says its session ended, waits for the app to sign in again.
 */
import { ParseError } from '#lib/wire/parse.ts';

// the session's module reaches for the page's history and storage, which a test of `split` has
// none of
const access = () => import('#lib/auth/access.ts');

/** the `data` of the complete messages at the start of `text`, and what is left of it */
export function split(text: string): { messages: string[]; rest: string } {
	const parts = text.replace(/\r\n?/g, '\n').split('\n\n');
	const rest = parts.pop()!;
	const messages: string[] = [];
	for (const part of parts) {
		const data: string[] = [];
		for (const line of part.split('\n')) {
			if (line.startsWith('data:')) data.push(line.slice(5).replace(/^ /, ''));
			else if (line === 'data') data.push('');
		}
		if (data.length) messages.push(data.join('\n'));
	}
	return { messages, rest };
}

/** how a stream stands: not answered yet, open, or waiting to try again */
export type Link = 'connecting' | 'live' | 'retrying';

const DELAYS = [500, 1000, 2000, 5000, 10000];

export interface Listener<T extends { kind: string }> {
	/** reads one message; throws a ParseError for one off the contract */
	parse(data: string): T;
	events(events: T[]): void;
	link(state: Link): void;
}

/** reads `url` until `stop` is called; returns `stop` */
export function stream<T extends { kind: string }>(url: string, on: Listener<T>): () => void {
	let abort: AbortController | null = null;
	let stopped = false;
	let failures = 0;
	let wake: (() => void) | null = null;

	const visible = () =>
		new Promise<void>((ok) => {
			if (!document.hidden) return ok();
			wake = ok;
		});
	const onVisibility = () => {
		if (document.hidden) abort?.abort();
		else wake?.();
	};
	document.addEventListener('visibilitychange', onVisibility);

	/** reads messages until the stream ends; true if it ended because the session did */
	async function read(body: ReadableStream<Uint8Array>) {
		const reader = body.getReader();
		const text = new TextDecoder();
		let rest = '';
		for (;;) {
			const { done, value } = await reader.read();
			if (done) return false;
			const got = split(rest + text.decode(value, { stream: true }));
			rest = got.rest;
			const events: T[] = [];
			let expired = false;
			for (const m of got.messages) {
				try {
					const e = on.parse(m);
					if (e.kind === 'expired') expired = true;
					else events.push(e);
				} catch (e) {
					if (!(e instanceof ParseError)) throw e;
					console.warn(`phoebe: dropped a message of ${url}: ${e.message}`);
				}
			}
			if (events.length) on.events(events);
			if (expired) return true;
		}
	}

	(async () => {
		while (!stopped) {
			await visible();
			if (stopped) return;
			abort = new AbortController();
			let ended = false;
			try {
				const res = await (await access()).signed(url, { signal: abort.signal });
				if (res.ok && res.body) {
					failures = 0;
					on.link('live');
					ended = await read(res.body);
				} else {
					ended = res.status === 401;
					await res.body?.cancel();
				}
			} catch {}
			if (stopped) return;
			if (ended) {
				on.link('connecting');
				abort.abort();
				await (await access()).refused();
				failures = 0;
				continue;
			}
			on.link('retrying');
			if (document.hidden) continue;
			const delay = DELAYS[Math.min(failures++, DELAYS.length - 1)] * (0.75 + Math.random() * 0.5);
			await new Promise((ok) => setTimeout(ok, delay));
		}
	})();

	return () => {
		stopped = true;
		abort?.abort();
		wake?.();
		document.removeEventListener('visibilitychange', onVisibility);
	};
}
