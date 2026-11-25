<!-- The app's glyphs. Strokes come from the rule weights; color is the text's. -->
<script lang="ts">
	let { name }: { name: 'chev' | 'back' | 'x' | 'search' | 'moon' | 'device' } = $props();
</script>

{#if name === 'chev'}
	<svg class="chev" width="7" height="12" viewBox="0 0 7 12" fill="none" aria-hidden="true">
		<path
			d="M1 1l5 5-5 5"
			stroke="currentColor"
			stroke-width="1.5"
			stroke-linecap="round"
			stroke-linejoin="round"
		/>
	</svg>
{:else if name === 'back'}
	<svg width="10" height="17" viewBox="0 0 10 17" fill="none" aria-hidden="true">
		<path
			d="M8.5 1.5L2 8.5l6.5 7"
			stroke="currentColor"
			stroke-width="2"
			stroke-linecap="round"
			stroke-linejoin="round"
		/>
	</svg>
{:else if name === 'x'}
	<svg width="14" height="14" viewBox="0 0 14 14" fill="none" aria-hidden="true">
		<path d="M2 2l10 10M12 2L2 12" stroke="currentColor" stroke-width="2" stroke-linecap="round" />
	</svg>
{:else if name === 'search'}
	<svg width="17" height="17" viewBox="0 0 17 17" fill="none" aria-hidden="true">
		<circle cx="7.5" cy="7.5" r="5.5" stroke="currentColor" stroke-width="1.5" /><path
			d="M11.5 11.5L15.5 15.5"
			stroke="currentColor"
			stroke-width="1.5"
			stroke-linecap="round"
		/>
	</svg>
{:else if name === 'device'}
	<svg width="17" height="17" viewBox="0 0 17 17" fill="none" aria-hidden="true">
		<path
			d="M5 1.5h7a1.5 1.5 0 0 1 1.5 1.5v11a1.5 1.5 0 0 1-1.5 1.5H5A1.5 1.5 0 0 1 3.5 14V3A1.5 1.5 0 0 1 5 1.5zM7.5 12.5h2"
			stroke="currentColor"
			stroke-width="1.5"
			stroke-linecap="round"
		/>
	</svg>
{:else}
	<!-- the mark: the moon's disc, and the crescent it shows -->
	<svg width="15" height="15" viewBox="0 0 24 24" aria-hidden="true">
		<circle cx="12" cy="12" r="9.5" fill="none" stroke="currentColor" stroke-width="2" /><path
			d="M12 5.6a6.4 6.4 0 0 0 0 12.8a4.6 6.4 0 0 1 0-12.8z"
			fill="currentColor"
		/>
	</svg>
{/if}
