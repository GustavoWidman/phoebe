<!-- a service: who reaches it, and the median latency from the nodes that do -->
<script lang="ts">
	import { latency } from '#lib/derive/format.ts';
	import type { ServiceStatus } from '#lib/derive/status.ts';
	import { href } from '#lib/router.svelte.ts';
	import Dot from '#lib/ui/Dot.svelte';
	import Pips from '#lib/ui/Pips.svelte';
	import type { Service } from '#lib/wire/types.ts';

	let { service: s, status: st }: { service: Service; status: ServiceStatus } = $props();
	const short = $derived(st.reached && st.reached.ok < st.reached.of);
	const unitFailed = $derived(s.unit?.state === 'failed');
</script>

<a class="row" href={href({ screen: 'service', name: s.name })} data-service={s.name}>
	<Dot s={st.dot} />
	<div>
		<div class="rh"><span class="nm m ell">{s.name}</span></div>
		<div class="sb" class:warn={short && st.dot === 'warn'} class:bad={st.dot === 'bad'}>
			{#if unitFailed}
				{s.unit?.name} failed
			{:else if short}
				{st.reached?.ok} of {st.reached?.of} nodes reach it
			{:else}
				{s.unit ? s.unit.name.replace(/\.service$/, '') : 'no unit'} · {s.machine}
			{/if}
		</div>
	</div>
	<span class="rt">
		{#if st.reached}
			<Pips dots={st.reach.map((r) => (r.ok === false ? 'bad' : r.ok ? 'ok' : 'idle'))} />
			<span class="lat">{latency(st.median)}</span>
		{:else}
			<span class:bad={unitFailed}>{s.unit?.state ?? ''}</span>
		{/if}
	</span>
</a>
