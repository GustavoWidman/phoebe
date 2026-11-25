<!--
	This device: the theme it shows, its session, its notifications, and the build and node it
	reads.
-->
<script lang="ts">
	import { account } from '#lib/auth/account.svelte.ts';
	import { name } from '#lib/auth/access.ts';
	import { date } from '#lib/derive/format.ts';
	import Choices from '#lib/ui/Choices.svelte';
	import Label from '#lib/ui/Label.svelte';
	import { THEMES, themes } from '#lib/ui/theme.svelte.ts';
	import type { Snapshot } from '#lib/wire/types.ts';

	let { snap }: { snap: Snapshot } = $props();

	const session = $derived(account.live ? account.session : null);
</script>

<div class="tb">
	<div class="k">Device</div>
	<h1>This device</h1>
	<p class="sub">How phoebe looks here, who it reads as, and what it tells you.</p>
</div>
<Label label="Theme" value={themes.theme === 'auto' ? 'follows the system' : themes.theme} />
<Choices options={THEMES} on={themes.theme} pick={(t) => themes.set(t)} />
<Label label="Session" value={session ? `until ${date(session.viewer.expires_at)}` : 'none'} />
<dl class="kv" data-session>
	{#if session}
		<div>
			<dt>account</dt>
			<dd>{session.viewer.login}</dd>
		</div>
		<div>
			<dt>device</dt>
			<dd class="m">{session.viewer.device}</dd>
		</div>
		<div>
			<dt>signed in via</dt>
			<dd><span class="m">{name(session.via)}</span> <span class="mute">· tailnet</span></dd>
		</div>
	{:else}
		<div>
			<dt>access</dt>
			<dd>Open to anyone who reaches this address</dd>
		</div>
	{/if}
</dl>
{#await import('#lib/push/Notifications.svelte') then m}
	<m.default name={session?.viewer.device ?? 'this device'} />
{/await}
<Label label="Served by" />
<dl class="kv">
	<div>
		<dt>node</dt>
		<dd class="m">{snap.served_by}</dd>
	</div>
	<div>
		<dt>schema</dt>
		<dd class="m">{snap.schema}</dd>
	</div>
</dl>
