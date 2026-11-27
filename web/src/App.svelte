<script lang="ts">
	import { onMount, type Component } from 'svelte';
	import { servicePath } from '#lib/api.svelte.ts';
	import { router, type Route } from '#lib/router.svelte.ts';
	import Missing from '#lib/screens/Missing.svelte';
	import Frame from '#lib/shell/Frame.svelte';
	import { fleet } from '#lib/stream/fleet.svelte.ts';
	import Dot from '#lib/ui/Dot.svelte';
	import Icon from '#lib/ui/Icon.svelte';

	/**
	 * Each screen is its own chunk, so a cold start loads the frame and the one screen it opens
	 * on; the rest load once the first is up.
	 */
	const SCREENS = {
		fleet: () => import('#lib/screens/Fleet.svelte'),
		machine: () => import('#lib/screens/Machine.svelte'),
		service: () => import('#lib/screens/Service.svelte'),
		alert: () => import('#lib/screens/Alert.svelte'),
		logs: () => import('#lib/screens/Logs.svelte'),
		device: () => import('#lib/screens/Device.svelte')
	} satisfies Record<
		Exclude<Route['screen'], 'signin'>,
		() => Promise<{ default: Component<never> }>
	>;
	type Screens = {
		[K in keyof typeof SCREENS]?: Awaited<ReturnType<(typeof SCREENS)[K]>>['default'];
	};

	let screens = $state.raw<Screens>({});
	const load = (name: keyof Screens) =>
		screens[name] ?? SCREENS[name]().then((m) => (screens = { ...screens, [name]: m.default }));

	const route = $derived(router.route);
	$effect(() => void (route.screen !== 'signin' && load(route.screen)));
	onMount(() => {
		const stop = fleet.connect();
		const idle = setTimeout(
			() => Object.keys(SCREENS).forEach((k) => load(k as keyof Screens)),
			2000
		);
		return () => (stop(), clearTimeout(idle));
	});
</script>

{#if route.screen === 'signin'}
	{#await import('#lib/auth/SignIn.svelte') then m}<m.default />{/await}
{:else if fleet.snapshot}
	{@const snap = fleet.snapshot}
	<Frame {route} {snap} link={fleet.link}>
		{#key route}
			{#if route.screen === 'fleet' && screens.fleet}
				<screens.fleet {snap} />
			{:else if route.screen === 'machine' && screens.machine}
				{@const m = fleet.machines.get(route.name as never)}
				{#if m}<screens.machine {snap} machine={m} />{:else}<Missing
						what="machine"
						name={route.name}
					/>{/if}
			{:else if route.screen === 'service' && screens.service}
				{@const s = fleet.services.get(servicePath(route))}
				{#if s}<screens.service {snap} service={s} />{:else}<Missing
						what="service"
						name="{route.name} on {route.machine}"
					/>{/if}
			{:else if route.screen === 'alert' && screens.alert}
				<screens.alert {snap} id={route.id} />
			{:else if route.screen === 'logs' && screens.logs}
				<screens.logs {snap} />
			{:else if route.screen === 'device' && screens.device}
				<screens.device {snap} />
			{/if}
		{/key}
	</Frame>
{:else}
	<!-- the shell, before any node has answered: offline, or still connecting -->
	<div class="ph">
		<nav class="nav"><span class="wm"><Icon name="moon" />phoebe</span></nav>
		<div class="scroll">
			<div class="tb">
				<h1>Fleet</h1>
				<div class="st"><Dot s="idle" />Waiting for a node to answer</div>
			</div>
		</div>
	</div>
{/if}
