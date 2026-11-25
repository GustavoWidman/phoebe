<!-- the one control for ranges and filters -->
<script lang="ts" generics="T extends string">
	let {
		options,
		on,
		label = (o: T) => o as string,
		pick
	}: { options: T[]; on: T; label?: (o: T) => string; pick: (o: T) => void } = $props();
</script>

<div class="choices" role="group">
	{#each options as o (o)}
		<button class="opt m" class:on={o === on} aria-pressed={o === on} onclick={() => pick(o)}>
			{label(o)}
		</button>
	{/each}
</div>
