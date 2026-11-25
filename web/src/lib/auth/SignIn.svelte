<!--
	Signing in: the app asks the nodes' tailnet addresses every few seconds and signs itself in
	through the first that answers, then says until when the session lasts. It sits outside the
	frame, before any fleet has loaded.
-->
<script lang="ts">
	import { onDestroy } from 'svelte';
	import { date } from '#lib/derive/format.ts';
	import { href, router } from '#lib/router.svelte.ts';
	import Dot from '#lib/ui/Dot.svelte';
	import Icon from '#lib/ui/Icon.svelte';
	import { noun, standalone as isStandalone } from '#lib/ui/platform.ts';
	import { back } from './access.ts';
	import { device as stored } from './device.ts';
	import { PROBING, hosts, name, next, ping, signIn, step, type Phase } from './signin.ts';

	const D = 864e5;
	const device = noun();
	const standalone = isStandalone();
	const after = back();

	let phase = $state.raw<Phase>(PROBING);
	/** a session this device held, which the server no longer takes */
	let expired = $state(false);
	let list: string[] | null = null;
	let addr = $state<string | null>(null);
	let copied = $state(false);
	let checked = $state<number | null>(null);
	let timer: ReturnType<typeof setTimeout> | undefined;
	let clock: ReturnType<typeof setInterval> | undefined;
	let gone = false;

	stored.session().then((s) => (expired = !!s && (!!s.ended || s.viewer.expires_at <= Date.now())));

	async function round() {
		clearTimeout(timer);
		if (gone || document.hidden) return;
		try {
			list ??= await hosts();
			addr = list[0] ?? null;
			const via = await ping(list);
			if (!via) phase = step(phase, { kind: 'unanswered' }, Date.now());
			else {
				phase = step(phase, { kind: 'answered', via }, Date.now());
				phase = step(phase, await signIn(via), Date.now());
			}
		} catch {
			phase = step(phase, { kind: 'failed', why: 'no phoebe node answered' }, Date.now());
		}
		const at = next(phase, Date.now());
		if (at !== null && !gone) timer = setTimeout(round, Math.max(0, at - Date.now()));
	}

	const again = () => {
		phase = PROBING;
		round();
	};
	const shown = () => {
		if (!document.hidden && phase.kind === 'probing') round();
	};
	document.addEventListener('visibilitychange', shown);
	clock = setInterval(() => {
		checked =
			phase.kind === 'probing' && phase.checked !== null
				? Math.max(0, Math.floor((Date.now() - phase.checked) / 1000))
				: null;
	}, 1000);
	onDestroy(() => {
		gone = true;
		clearTimeout(timer);
		clearInterval(clock);
		document.removeEventListener('visibilitychange', shown);
	});
	round();

	async function copy(e: MouseEvent) {
		e.preventDefault();
		if (!addr) return;
		copied = true;
		setTimeout(() => !gone && (copied = false), 1600);
		try {
			await navigator.clipboard.writeText(addr);
		} catch {}
	}

	const open = (e: MouseEvent) => (e.preventDefault(), router.go(after, true));
	const where = after.screen === 'fleet' ? 'the fleet' : 'where you were';
</script>

<div class="ph">
	<div class="scroll">
		<div class="auth">
			<span class="wm"><Icon name="moon" />phoebe</span>
			{#if phase.kind === 'signed_in'}
				{@const s = phase.session}
				<div class="tb">
					<h1>Signed in on the tailnet</h1>
					<p class="sub">
						This {device} can open phoebe from anywhere until
						<span class="m">{date(s.viewer.expires_at)}</span>.
					</p>
				</div>
				<dl class="kv">
					<div>
						<dt>account</dt>
						<dd>{s.viewer.login}</dd>
					</div>
					<div>
						<dt>device</dt>
						<dd>
							<span class="m">{s.viewer.device}</span>
							<span class="mute">· {standalone ? 'Home Screen app' : 'browser'}</span>
						</dd>
					</div>
					<div>
						<dt>signed in via</dt>
						<dd><span class="m">{name(s.via)}</span> <span class="mute">· tailnet</span></dd>
					</div>
					<div>
						<dt>session</dt>
						<dd>
							{Math.round((s.viewer.expires_at - s.iat) / D)} days
							<span class="mute">· then sign in on the tailnet again</span>
						</dd>
					</div>
				</dl>
				<a class="btn" href={href(after)} onclick={open}>Open {where}</a>
				{#await import('#lib/push/Notifications.svelte') then m}
					<m.default name={s.viewer.device} signin />
				{/await}
			{:else}
				<div class="tb">
					{#if phase.kind === 'refused'}
						<h1>This {device} may not sign in</h1>
						<p class="sub">
							{name(phase.via)} answered on the tailnet, but the tailnet policy grants this {device}
							no access to phoebe. Once it does, try again.
						</p>
					{:else if expired}
						<h1>Your session ended</h1>
						<p class="sub">
							A session lasts 7 days from the last time this {device} was on the tailnet. Turn on Tailscale
							and phoebe signs in again by itself.
						</p>
					{:else}
						<h1>Open this on your tailnet to sign in</h1>
						<p class="sub">
							This address is public, so it cannot start a session. Sign in once from a device on
							the tailnet; the session then works from anywhere for 7 days.
						</p>
					{/if}
				</div>
				{#if phase.kind !== 'refused'}
					<ol class="steps">
						<li><span class="m">01</span>Turn on Tailscale on this {device}.</li>
						<li><span class="m">02</span>Let phoebe ask the tailnet address below.</li>
						<li><span class="m">03</span>It signs in by itself, and works anywhere after.</li>
					</ol>
				{/if}
				<a class="addr" href={addr ?? '/signin'} onclick={copy}>
					<span class="m ell">{addr ?? 'asking for the tailnet address…'}</span>
					<span class="cp m">{copied ? 'copied' : 'copy'}</span>
				</a>
				<p class="probe m">
					{#if phase.kind === 'signing'}
						<Dot s="acc" /><span>on the tailnet · signing in through {name(phase.via)}</span>
					{:else if phase.kind === 'refused'}
						<Dot s="bad" /><span>refused by {name(phase.via)} · {phase.why}</span>
					{:else if phase.checked === null}
						<Dot s="idle" /><span>asking the tailnet…</span>
					{:else}
						<Dot s="idle" /><span>
							{phase.failure ?? 'not on the tailnet'} · checked {checked ?? 0}s ago
						</span>
					{/if}
				</p>
				{#if phase.kind === 'refused'}
					<a class="btn" href="/signin" onclick={(e) => (e.preventDefault(), again())}>Try again</a>
				{/if}
			{/if}
		</div>
	</div>
</div>
