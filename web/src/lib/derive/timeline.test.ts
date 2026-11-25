import { expect, test } from 'vitest';
import type { Hostname, TimelineEntry } from '#lib/wire/types.ts';
import { events } from './timeline.ts';

const h = (s: string) => s as Hostname;
const at = (m: number) => 1_795_629_000_000 + m * 1000;

test('a run of one kind across the nodes reads as one event', () => {
	const entries: TimelineEntry[] = [
		{ kind: 'pending', at: at(0), node: h('oracle-1') },
		{ kind: 'pending', at: at(4), node: h('lab') },
		{ kind: 'firing', at: at(60), node: h('oracle-1') },
		{ kind: 'firing', at: at(64), node: h('lab') },
		{ kind: 'firing', at: at(67), node: h('oracle-2') },
		{ kind: 'firing', at: at(71), node: h('altar') },
		{ kind: 'notified', at: at(90), by: h('oracle-2'), resolved: false },
		{ kind: 'resolved', at: at(900), node: h('oracle-1') },
		{ kind: 'resolved', at: at(901), node: h('lab') },
		{ kind: 'resolved', at: at(902), node: h('oracle-2') },
		{ kind: 'resolved', at: at(903), node: h('altar') }
	];
	expect(events(entries, 4)).toEqual([
		{ at: at(0), dot: 'off', text: 'Pending on oracle-1, then lab' },
		{ at: at(60), dot: 'bad', text: 'Fires on oracle-1, then the 3 other nodes' },
		{ at: at(90), dot: 'off', text: 'Notification sent by oracle-2' },
		{ at: at(900), dot: 'ok', text: 'Resolved on all 4 nodes' }
	]);
});
