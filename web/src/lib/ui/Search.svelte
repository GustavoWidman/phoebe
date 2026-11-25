<script lang="ts">
	import type { Link } from '#lib/stream/sse.ts';
	import Dot from './Dot.svelte';
	import Icon from './Icon.svelte';

	let { value = $bindable(''), link }: { value?: string; link: Link } = $props();
</script>

<label class="search">
	<Icon name="search" />
	<input bind:value type="search" placeholder="search the fleet" aria-label="search the logs" />
	<span class="live"><Dot s={link === 'live' ? 'ok' : 'idle'} />live</span>
</label>
