<!-- a machine: name, kind, and its gauges while up, or since when it rests -->
<script lang="ts">
	import { pct } from '#lib/derive/format.ts';
	import { machineDot, presence } from '#lib/derive/status.ts';
	import { href } from '#lib/router.svelte.ts';
	import Dot from '#lib/ui/Dot.svelte';
	import Gauge from '#lib/ui/Gauge.svelte';
	import Icon from '#lib/ui/Icon.svelte';
	import type { Machine } from '#lib/wire/types.ts';

	let { machine: m, now }: { machine: Machine; now: number } = $props();
	const away = $derived(presence(m, now));
</script>

<a
	class="row"
	class:muted={m.state.kind === 'resting'}
	href={href({ screen: 'machine', name: m.name })}
	data-machine={m.name}
>
	<Dot s={machineDot(m)} />
	<div>
		<div class="rh">
			<span class="nm m">{m.name}</span><span class="pl ell">
				{m.os}
				{m.always_on ? 'server' : 'desktop'}{m.hardware ? ` · ${m.hardware.arch}` : ''}
			</span>
		</div>
		{#if m.state.kind === 'up'}
			<div class="gs">
				{#each [['cpu', m.state.vitals.cpu], ['mem', m.state.vitals.mem], ['disk', m.state.vitals.disk]] as const as [k, v] (k)}
					<div class="g">{k} <b>{pct(v)}</b><Gauge {v} /></div>
				{/each}
			</div>
			{#if m.state.units.failed.length}
				<div class="sb bad">{m.state.units.failed.map((u) => u.name).join(', ')} failed</div>
			{/if}
		{:else}
			<div class="sb" class:bad={m.state.kind === 'down'}>{away}</div>
		{/if}
	</div>
	<span class="rt"><Icon name="chev" /></span>
</a>
