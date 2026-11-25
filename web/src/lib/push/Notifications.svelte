<!--
	Notifications on this device: turning them on, the subscription to add to the list phoebe
	pushes to, and whether the list has it. It loads only where it shows, on the device screen
	and the signed-in page, outside every screen's first load.
-->
<script lang="ts">
	import { onDestroy, untrack } from 'svelte';
	import { signed } from '#lib/auth/access.ts';
	import Dot from '#lib/ui/Dot.svelte';
	import Label from '#lib/ui/Label.svelte';
	import { ios, noun, standalone } from '#lib/ui/platform.ts';
	import { parsePushView } from '#lib/wire/parse.ts';
	import { changed } from './changed.ts';
	import {
		QUIET,
		b64url,
		bytes,
		control,
		endpointHash,
		entry,
		pretty,
		type Browser,
		type Control,
		type Local,
		type Mine,
		type Server
	} from './state.ts';

	/** `name` is the device's name in the session, which names its entry in the list */
	let { name, signin = false }: { name: string; signin?: boolean } = $props();

	const word = noun();
	const supported = 'PushManager' in window && 'serviceWorker' in navigator;
	const permission = () => ('Notification' in window ? Notification.permission : 'default');

	let browser = $state.raw<Browser>({
		push: supported,
		ios: ios(),
		standalone: standalone(),
		permission: permission()
	});
	let server = $state.raw<Server>({ kind: 'loading' });
	let mine = $state.raw<Mine | null>(null);
	let local = $state.raw<Local>(QUIET);
	/** the subscription has been read, or there is none to read */
	let read = $state(!supported);
	let copied = $state(false);
	let registration: ServiceWorkerRegistration | null = null;
	/** the served key, decoded while the screen opens so a tap subscribes at once */
	let key: Uint8Array<ArrayBuffer> | null = null;
	let gone = false;

	const c: Control = $derived(read ? control(server, browser, mine, local) : { kind: 'loading' });
	const edit = $derived(server.kind === 'ok' ? server.push.edit : null);

	async function ask() {
		try {
			const res = await signed('/api/push');
			if (res.status === 404) return void (server = { kind: 'none' });
			if (!res.ok)
				return void (server = { kind: 'failed', why: `the server answered ${res.status}` });
			const push = parsePushView(await res.json(), 'push');
			key = bytes(push.key);
			server = { kind: 'ok', push };
		} catch (e) {
			server = { kind: 'failed', why: e instanceof Error ? e.message : String(e) };
		}
	}

	async function look(sub: PushSubscription | null) {
		const e = sub && entry(name, sub.toJSON());
		const k = sub?.options?.applicationServerKey;
		mine = e ? { entry: e, hash: await endpointHash(e.endpoint), key: k ? b64url(k) : null } : null;
	}

	async function start() {
		if (!supported) return;
		registration = await navigator.serviceWorker.ready;
		const [sub, was] = await Promise.all([
			registration.pushManager.getSubscription(),
			changed.read()
		]);
		await look(sub);
		local = { ...local, changed: was };
		read = true;
	}

	const why = (e: unknown) => (e instanceof Error ? e.message : String(e));

	function subscribe() {
		return registration!.pushManager.subscribe({
			userVisibleOnly: true,
			applicationServerKey: key!
		});
	}

	// Safari lets a page ask only while it handles a tap, so this subscribes before anything else
	function on() {
		if (!registration || !key) return;
		const asked = subscribe();
		local = { ...local, asking: true, failed: null, removed: null };
		asked
			.then(look, (e) => {
				if (permission() !== 'denied') local = { ...local, failed: why(e) };
			})
			.finally(() => {
				local = { ...local, asking: false };
				browser = { ...browser, permission: permission() };
			});
	}

	async function off() {
		const listed = c.kind === 'on' || c.kind === 'stale' || c.kind === 'unknown';
		const sub = await registration?.pushManager.getSubscription();
		try {
			await sub?.unsubscribe();
		} catch (e) {
			return void (local = { ...local, failed: why(e) });
		}
		mine = null;
		local = { ...local, changed: false, removed: listed ? name : null, failed: null };
		changed.clear().catch(() => {});
	}

	let renewing = false;

	/** a subscription made under a key the fleet no longer sends with, made again under its key */
	async function renew() {
		if (renewing) return;
		renewing = true;
		try {
			await (await registration!.pushManager.getSubscription())?.unsubscribe();
			const sub = await subscribe();
			await changed.mark().catch(() => {});
			local = { ...local, changed: true };
			await look(sub);
		} catch (e) {
			mine = null;
			local = { ...local, failed: why(e) };
		} finally {
			renewing = false;
		}
	}

	$effect(() => {
		if (c.kind === 'rotated') renew();
		else if (c.kind === 'on' && local.changed) {
			local = { ...local, changed: false };
			changed.clear().catch(() => {});
		}
	});

	async function copy(text: string) {
		copied = true;
		setTimeout(() => !gone && (copied = false), 1600);
		try {
			await navigator.clipboard.writeText(text);
		} catch {}
	}

	/** the dot rings when the device turns on while the control is up, not when it opens on */
	let ring = $state(false);
	let last: Control['kind'] = 'loading';
	$effect(() => {
		const kind = c.kind;
		untrack(() => {
			if (kind !== last) ring = kind === 'on' && last !== 'loading';
			last = kind;
		});
	});

	// while away, the person may have changed the permission in Settings, or committed the entry
	// and deployed
	const shown = () => {
		if (document.hidden) return;
		browser = { ...browser, permission: permission() };
		if (server.kind !== 'loading') ask();
		if (registration) registration.pushManager.getSubscription().then(look);
	};
	document.addEventListener('visibilitychange', shown);
	onDestroy(() => {
		gone = true;
		document.removeEventListener('visibilitychange', shown);
	});

	ask();
	start();

	const dot = (c: Control) =>
		c.kind === 'on'
			? 'ok'
			: c.kind === 'denied' || c.kind === 'stale' || c.kind === 'unknown'
				? 'warn'
				: c.kind === 'asking' || c.kind === 'uncommitted'
					? 'acc'
					: 'idle';
</script>

{#snippet block(text: string, what: string)}
	<div class="entry">
		<pre class="m">{text}</pre>
		<button class="cp m" onclick={() => copy(text)}>{copied ? 'copied' : 'copy'}</button>
	</div>
	<p class="cmd m">
		{#if edit}{edit}{' '}{/if}<span class="mute"># {what}</span>
	</p>
{/snippet}

{#snippet removed(entry: string)}
	<p class="note">
		This {word} gets no more notifications. The list still has <b class="m">"{entry}"</b>: remove
		it, then deploy.
	</p>
	{#if edit}<p class="cmd m">{edit}</p>{/if}
{/snippet}

{#if signin}
	{#if c.kind === 'off' || c.kind === 'asking'}
		<button class="btn q" disabled={c.kind === 'asking'} onclick={on}>
			{c.kind === 'asking' ? 'Asking…' : 'Turn on notifications'}
		</button>
		{#if c.kind === 'off' && c.failed}<p class="probe m bad">{c.failed}</p>{/if}
	{:else if c.kind !== 'loading' && c.kind !== 'absent' && c.kind !== 'rotated'}
		<div class="probe m" data-push={c.kind}>
			<span class="d" class:ring><Dot s={dot(c)} /></span><span>
				{#if c.kind === 'on'}
					notifications on · this {word} is in the list
				{:else if c.kind === 'denied'}
					notifications off in {browser.ios ? 'Settings › Notifications › phoebe' : 'site settings'}
				{:else if c.kind === 'install'}
					add phoebe to the Home Screen to get notifications
				{:else if c.kind === 'stale'}
					this {word}'s subscription changed
				{:else if c.kind === 'uncommitted'}
					subscribed · add this entry to the list, then deploy
				{:else if c.kind === 'unknown'}
					subscribed · {c.why}
				{/if}
			</span>
		</div>
		{#if c.kind === 'uncommitted' || c.kind === 'stale'}
			{@render block(
				pretty(c.entry),
				c.kind === 'stale' ? 'put it in place of the old entry' : 'add this entry to the array'
			)}
		{/if}
	{/if}
{:else if c.kind !== 'absent'}
	<section class="push" aria-busy={server.kind === 'loading'} data-push={c.kind}>
		<Label label="Notifications" />
		{#if c.kind === 'loading' || c.kind === 'rotated'}
			<div class="pr">
				<Dot s="idle" />
				<div><b class="mute">…</b></div>
			</div>
		{:else}
			<div class="pr">
				<span class="d" class:ring><Dot s={dot(c)} /></span>
				<div>
					{#if c.kind === 'off'}
						<b>Off</b>
						<p>Every page alert, as it fires and as it resolves.</p>
					{:else if c.kind === 'asking'}
						<b>Asking…</b>
						<p>Allow notifications in the prompt.</p>
					{:else if c.kind === 'denied'}
						<b>Off in {browser.ios ? 'Settings' : 'site settings'}</b>
						<p>
							{browser.ios
								? 'Turn them on in Settings › Notifications › phoebe.'
								: 'This browser blocks them for this site; allow them in its site settings.'}
						</p>
					{:else if c.kind === 'install'}
						<b>Add phoebe to the Home Screen to get notifications</b>
						<p>In Safari, Share › Add to Home Screen, then open phoebe from there.</p>
					{:else if c.kind === 'uncommitted'}
						<b>Subscribed · not in the list yet</b>
						<p>Add this entry to the subscription list, then deploy.</p>
					{:else if c.kind === 'stale'}
						<b>This {word}'s subscription changed</b>
						<p>Put this entry in place of its old one in the list, then deploy.</p>
					{:else if c.kind === 'on'}
						<b>On</b>
						<p>Every page alert on this {word}, as it fires and as it resolves.</p>
					{:else if c.kind === 'unknown'}
						<b>Subscribed</b>
						<p>Whether the list has it is unknown: {c.why}.</p>
					{/if}
				</div>
				{#if c.kind === 'off' || c.kind === 'asking'}
					<button class="opt" disabled={c.kind === 'asking'} onclick={on}>Turn on</button>
				{:else if c.kind === 'on' || c.kind === 'uncommitted' || c.kind === 'stale' || c.kind === 'unknown'}
					<button class="opt" onclick={off}>Turn off</button>
				{/if}
			</div>
			{#if c.kind === 'off' && c.failed}<p class="note bad">{c.failed}</p>{/if}
			{#if c.kind === 'off' && c.removed}{@render removed(c.removed)}{/if}
			{#if c.kind === 'uncommitted' || c.kind === 'stale'}
				{@render block(
					pretty(c.entry),
					c.kind === 'stale' ? 'put it in place of the old entry' : 'add this entry to the array'
				)}
			{/if}
		{/if}
	</section>
{/if}
