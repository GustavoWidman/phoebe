<script lang="ts">
	import type { Dot } from '#lib/derive/status.ts';

	let { s, pulse = false }: { s: Dot; pulse?: boolean } = $props();
</script>

<span class="dot" class:pulse data-s={s}></span>
