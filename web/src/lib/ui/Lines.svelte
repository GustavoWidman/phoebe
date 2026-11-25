<!--
	Log lines in one size: time, with its date above it when that is not today, then host and
	unit (their own columns on desktop), then the text, in warn-ink for a warning and bad for an
	error, with the search's match washed.
-->
<script lang="ts">
	import { hms, otherDay } from '#lib/derive/format.ts';
	import { fleet } from '#lib/stream/fleet.svelte.ts';
	import type { Shown } from '#lib/log/feed.svelte.ts';
	import type { Level } from '#lib/wire/types.ts';

	let { lines, q = '', wide = false }: { lines: Shown[]; q?: string; wide?: boolean } = $props();

	const TONE: Record<Level, string> = { error: 'err', warn: 'warn', info: '', debug: '' };

	/** the text before, at and after the first match of `q` */
	function cut(text: string): [string, string, string] {
		const i = q ? text.toLowerCase().indexOf(q.toLowerCase()) : -1;
		return i < 0
			? [text, '', '']
			: [text.slice(0, i), text.slice(i, i + q.length), text.slice(i + q.length)];
	}
	const unitName = (u: string) => u.replace(/\.service$/, '');
	/** today is the fleet's, which a simulated fleet sets */
	const now = $derived(fleet.snapshot?.at ?? Date.now());
</script>

<div class="logs" class:wide>
	{#each lines as l (l.key)}
		{@const [a, m, b] = cut(l.text)}
		{@const day = otherDay(l.at, now)}
		<div class="l {TONE[l.level]}" class:new={l.fresh}>
			<span class="t">
				{#if day}<span class="d">{day}</span>{/if}{hms(l.at)}
			</span>
			{#if wide}
				<span class="h">{l.machine}</span><span class="u ell">{unitName(l.unit)}</span><span
					class="x"
				>
					{a}{#if m}<mark>{m}</mark>{/if}{b}
				</span>
			{:else}
				<span class="x">
					<span class="u">{`${l.machine} ${unitName(l.unit)} `}</span>{a}{#if m}<mark>
							{m}
						</mark>{/if}{b}
				</span>
			{/if}
		</div>
	{/each}
</div>
