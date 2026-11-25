<!-- LABEL ───── value -->
<script lang="ts">
	let {
		label,
		value = '',
		tone = ''
	}: { label: string; value?: string | number; tone?: '' | 'bad' | 'warn' } = $props();
</script>

<div class="lbl">
	{label}
	<hr />
	{#if value !== ''}<span class="v {tone}">{value}</span>{/if}
</div>
