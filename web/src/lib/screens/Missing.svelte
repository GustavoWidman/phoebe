<script lang="ts">
	let { what, name }: { what: string; name: string } = $props();
</script>

<div class="tb">
	<div class="k">{what}</div>
	<h1 class="m">{name}</h1>
	<p class="sub">The registry names no {what} by that name.</p>
</div>
