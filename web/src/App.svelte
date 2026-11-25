<script lang="ts">
	import { onMount } from 'svelte';
	import { router } from '#lib/router.svelte.ts';
	import Alert from '#lib/screens/Alert.svelte';
	import Device from '#lib/screens/Device.svelte';
	import Fleet from '#lib/screens/Fleet.svelte';
	import Logs from '#lib/screens/Logs.svelte';
	import Machine from '#lib/screens/Machine.svelte';
	import Missing from '#lib/screens/Missing.svelte';
	import Service from '#lib/screens/Service.svelte';
	import Frame from '#lib/shell/Frame.svelte';
	import { fleet } from '#lib/stream/fleet.svelte.ts';

	onMount(() => fleet.connect());
	const route = $derived(router.route);
</script>

{#if fleet.snapshot}
	{@const snap = fleet.snapshot}
	<Frame {route} {snap} link={fleet.link}>
		{#key route}
			{#if route.screen === 'fleet'}
				<Fleet {snap} />
			{:else if route.screen === 'machine'}
				{@const m = fleet.machines.get(route.name as never)}
				{#if m}<Machine {snap} machine={m} />{:else}<Missing
						what="machine"
						name={route.name}
					/>{/if}
			{:else if route.screen === 'service'}
				{@const s = fleet.services.get(route.name)}
				{#if s}<Service {snap} service={s} />{:else}<Missing
						what="service"
						name={route.name}
					/>{/if}
			{:else if route.screen === 'alert'}
				<Alert {snap} id={route.id} />
			{:else if route.screen === 'logs'}
				<Logs {snap} />
			{:else}
				<Device {snap} />
			{/if}
		{/key}
	</Frame>
{/if}
