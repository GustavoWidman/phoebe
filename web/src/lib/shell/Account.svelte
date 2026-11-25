<!--
	The account slot of the header: who this device signed in as, by the first letter of the
	login, or this device's glyph while it holds no session. It opens the device screen.
-->
<script lang="ts">
	import { account } from '#lib/auth/account.svelte.ts';
	import { href } from '#lib/router.svelte.ts';
	import Icon from '#lib/ui/Icon.svelte';

	const login = $derived(account.live ? account.session!.viewer.login : null);
</script>

<a
	class="ic"
	href={href({ screen: 'device' })}
	aria-label={login ? `this device, signed in as ${login}` : 'this device'}
	data-account
>
	{#if login}<span class="me">{login.charAt(0).toUpperCase()}</span>{:else}<Icon
			name="device"
		/>{/if}
</a>
