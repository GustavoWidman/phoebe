<!--
	Every machine's logs: a search, a machine to narrow to, and the live tail of what matches,
	newest at the bottom. Search and filters stay put while the lines scroll.
-->
<script lang="ts">
	import { tick } from 'svelte';
	import { LogFeed } from '#lib/log/feed.svelte.ts';
	import Choices from '#lib/ui/Choices.svelte';
	import Lines from '#lib/ui/Lines.svelte';
	import Search from '#lib/ui/Search.svelte';
	import { layout } from '#lib/ui/layout.svelte.ts';
	import type { Snapshot } from '#lib/wire/types.ts';

	let { snap }: { snap: Snapshot } = $props();

	const ALL = 'all machines';
	let typed = $state('');
	let q = $state('');
	let machine = $state(ALL);
	$effect(() => {
		const next = typed.trim();
		const wait = setTimeout(() => (q = next), 250);
		return () => clearTimeout(wait);
	});

	const feed = new LogFeed(
		() => ({ q, machine: machine === ALL ? undefined : machine }),
		() => true,
		layout.desk ? 200 : 120
	);

	let end = $state<HTMLElement>();
	let pinned = true;
	$effect(() => {
		void feed.lines;
		const scroller = end?.closest('.scroll, .main');
		if (!scroller || !pinned) return;
		tick().then(() => (scroller.scrollTop = scroller.scrollHeight));
	});
	$effect(() => {
		const scroller = end?.closest('.scroll, .main');
		if (!scroller) return;
		const onScroll = () =>
			(pinned = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 48);
		scroller.addEventListener('scroll', onScroll);
		return () => scroller.removeEventListener('scroll', onScroll);
	});
</script>

<div class="lh">
	{#if layout.desk}
		<div class="tb">
			<div class="k">Fleet · live</div>
			<h1>Logs</h1>
		</div>
	{/if}
	<Search bind:value={typed} link={feed.link} />
	<Choices
		options={[ALL, ...snap.machines.map((m) => m.name)]}
		on={machine}
		pick={(m) => (machine = m)}
	/>
</div>
<Lines lines={feed.lines} {q} wide={layout.desk} />
{#if !feed.lines.length}<p class="sub">No lines{q ? ` match “${q}”` : ''} yet.</p>{/if}
<div bind:this={end}></div>
