<!-- a fraction as a 2px track and fill, which turns bad past 85% -->
<script lang="ts">
	let { v }: { v: number } = $props();
</script>

<div class="gauge" class:hot={v > 0.85}><i style:--p={v.toFixed(3)}></i></div>
