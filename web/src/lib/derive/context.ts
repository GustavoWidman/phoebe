/**
 * What the frame around a screen shows for it: the title the nav bar fades in, the logs the
 * live tail and the log sheet follow, and whether the nav takes an alert's severity wash.
 */
import type { LogFilter } from '#lib/api.svelte.ts';
import type { Route } from '#lib/router.svelte.ts';
import type { Snapshot } from '#lib/wire/types.ts';
import { TREATMENT, alertTitle, type Treatment } from './status.ts';

export type Context = {
	title: string;
	/** what the tail and the sheet show; null on the logs screen, which is all logs */
	logs: { filter: LogFilter; say: string } | null;
	/** the wash the nav bar takes, from the firing alert under it */
	sev: Treatment | null;
	/** how the nav bar leaves the screen: from the root there is nowhere to go */
	leave: 'none' | 'back' | 'close';
};

const ALL = { filter: {}, say: 'all machines' };

export function context(route: Route, snap: Snapshot | null): Context {
	switch (route.screen) {
		case 'fleet':
			return { title: '', logs: ALL, sev: null, leave: 'none' };
		case 'machine':
			return {
				title: route.name,
				logs: { filter: { machine: route.name }, say: route.name },
				sev: null,
				leave: 'back'
			};
		case 'service':
			return {
				title: route.name,
				logs: { filter: { service: route.name }, say: route.name },
				sev: null,
				leave: 'back'
			};
		case 'alert': {
			const a = snap?.alerts.find((x) => x.id === route.id);
			const filter = a
				? a.service
					? { service: a.service }
					: a.machine
						? { machine: a.machine }
						: {}
				: {};
			return {
				title: a ? alertTitle(a) : 'Alert',
				logs: { filter, say: a ? alertTitle(a) : 'all machines' },
				sev: a?.state.kind === 'firing' ? TREATMENT[a.severity] : null,
				leave: 'close'
			};
		}
		case 'logs':
			return { title: 'Logs', logs: null, sev: null, leave: 'back' };
		case 'device':
			return { title: 'This device', logs: ALL, sev: null, leave: 'back' };
	}
}
