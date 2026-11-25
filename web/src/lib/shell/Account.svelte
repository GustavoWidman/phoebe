<!--
	The account slot of the header: this device, its session and its notifications. It opens
	the device screen.
-->
<script lang="ts">
	import { href } from '#lib/router.svelte.ts';
	import Icon from '#lib/ui/Icon.svelte';
</script>

<a class="ic" href={href({ screen: 'device' })} aria-label="this device" data-account>
	<Icon name="device" />
</a>
