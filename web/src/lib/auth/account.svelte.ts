/** This device's session, as the screens show it: who signed in, on which device, until when. */
import { device, type Session } from './device.ts';

class Account {
	session = $state.raw<Session | null>(null);
	/** a session the server still takes, as far as this device knows */
	live = $derived(!!this.session && !this.session.ended);
}

export const account = new Account();
device.watch((session) => (account.session = session));
