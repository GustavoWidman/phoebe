<!--
	This device: the theme it shows, and the build and node it reads. Its session and its
	notifications belong here too, once the app signs in.
-->
<script lang="ts">
	import Choices from '#lib/ui/Choices.svelte';
	import Label from '#lib/ui/Label.svelte';
	import { THEMES, themes } from '#lib/ui/theme.svelte.ts';
	import type { Snapshot } from '#lib/wire/types.ts';

	let { snap }: { snap: Snapshot } = $props();
</script>

<div class="tb">
	<div class="k">Device</div>
	<h1>This device</h1>
	<p class="sub">How phoebe looks here, and where it reads the fleet from.</p>
</div>
<Label label="Theme" value={themes.theme === 'auto' ? 'follows the system' : themes.theme} />
<Choices options={THEMES} on={themes.theme} pick={(t) => themes.set(t)} />
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
