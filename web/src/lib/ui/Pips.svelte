<!-- one dot per probing node: who sees it -->
<script lang="ts">
	import type { Dot as State } from '#lib/derive/status.ts';
	import Dot from './Dot.svelte';

	let { dots }: { dots: State[] } = $props();
</script>

<span class="pips">
	{#each dots as s, i (i)}<Dot {s} />{/each}
</span>
