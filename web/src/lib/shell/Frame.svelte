<!--
	What every screen sits in. Desktop: the sidebar, the screen, and the live tail of the logs
	it shows. Phone: the nav bar, the screen, and the log dock, which raises the log sheet.
-->
<script lang="ts">
	import { tick, type Snippet } from 'svelte';
	import { hm } from '#lib/derive/format.ts';
	import { context } from '#lib/derive/context.ts';
	import { LogFeed } from '#lib/log/feed.svelte.ts';
	import { router, type Route } from '#lib/router.svelte.ts';
	import type { Link } from '#lib/stream/sse.ts';
	import Dot from '#lib/ui/Dot.svelte';
	import Icon from '#lib/ui/Icon.svelte';
	import Lines from '#lib/ui/Lines.svelte';
	import { layout } from '#lib/ui/layout.svelte.ts';
	import type { Snapshot } from '#lib/wire/types.ts';
	import Account from './Account.svelte';
	import Side from './Side.svelte';

	let {
		route,
		snap,
		link,
		children
	}: { route: Route; snap: Snapshot; link: Link; children: Snippet } = $props();

	const ctx = $derived(context(route, snap));
	let sheet = $state(false);
	let scrolled = $state(false);
	let scroller = $state<HTMLElement>();
	let tail = $state<HTMLElement>();

	const feed = new LogFeed(
		() => (ctx.logs && (layout.desk || sheet) ? ctx.logs.filter : null),
		() => true,
		layout.desk ? 90 : 60
	);

	// a new screen starts at its top, with the sheet down
	let shown = '';
	$effect.pre(() => {
		const key = JSON.stringify(route);
		if (key === shown) return;
		shown = key;
		sheet = false;
		scrolled = false;
		tick().then(() => scroller?.scrollTo(0, 0));
	});

	// the tail follows the newest line while the reader is at the bottom
	let pinned = true;
	$effect(() => {
		void feed.lines;
		if (!tail || !pinned) return;
		tick().then(() => tail && (tail.scrollTop = tail.scrollHeight));
	});
	const onTailScroll = (e: Event) => {
		const el = e.currentTarget as HTMLElement;
		pinned = el.scrollHeight - el.scrollTop - el.clientHeight < 48;
	};

	const live = $derived<'ok' | 'idle'>(link === 'live' ? 'ok' : 'idle');
</script>

{#if layout.desk}
	<div class="desk" class:wide={!ctx.logs}>
		<aside class="side"><Side {route} {snap} /></aside>
		<main class="main" bind:this={scroller}><div class="in">{@render children()}</div></main>
		{#if ctx.logs}
			<aside class="tail" aria-label="live tail">
				<div class="tail-h">
					<div class="ell">
						<b>Live tail</b>
						<div class="s ell">{ctx.logs.say}</div>
					</div>
					<span class="live"><Dot s={live} />live</span>
				</div>
				<div class="tail-b" bind:this={tail} onscroll={onTailScroll}>
					<Lines lines={feed.lines} />
				</div>
			</aside>
		{/if}
	</div>
{:else}
	<div class="ph">
		<nav
			class="nav"
			class:scrolled={scrolled || route.screen === 'logs'}
			class:sev={ctx.sev}
			class:sev-warn={ctx.sev === 'warn'}
		>
			{#if ctx.leave === 'none'}
				<span class="wm"><Icon name="moon" />phoebe</span>
			{:else}
				<button class="back" onclick={() => router.back({ screen: 'fleet' })}>
					<Icon name={ctx.leave === 'close' ? 'x' : 'back'} /><span>Fleet</span>
				</button>
			{/if}
			<span class="nt ell">{ctx.title}</span>
			<span class="nr">
				<span class="live m"><Dot s={live} />{hm(snap.at)}</span>
				{#if ctx.leave === 'none'}
					<a class="ic" href="/logs" aria-label="search the logs"><Icon name="search" /></a>
					<Account />
				{/if}
			</span>
		</nav>
		<div
			class="scroll"
			bind:this={scroller}
			onscroll={(e) => (scrolled = e.currentTarget.scrollTop > 24)}
		>
			{@render children()}
		</div>
		{#if ctx.logs}
			<button class="dock" onclick={() => (sheet = true)} aria-expanded={sheet}>
				<Dot s={live} /><b>Logs</b><span class="s ell">{ctx.logs.say} · live</span><span class="up">
					<Icon name="chev" />
				</span>
			</button>
			<div class="sw" class:open={sheet} aria-hidden={!sheet}>
				<button
					class="scrim"
					aria-label="close the logs"
					tabindex="-1"
					onclick={() => (sheet = false)}
				></button>
				<div class="sheet" role="dialog" aria-label="logs">
					<span class="grab"></span>
					<div class="sh-h">
						<div class="ell">
							<div class="t">Logs</div>
							<div class="s ell">{ctx.logs.say} · live</div>
						</div>
						<button class="done" onclick={() => (sheet = false)}>Done</button>
					</div>
					<div class="sh-b" bind:this={tail} onscroll={onTailScroll}>
						<Lines lines={feed.lines} />
					</div>
				</div>
			</div>
		{/if}
	</div>
{/if}
