/**
 * The theme this device chose: the system's, or light or dark whatever the system says. It is
 * kept on the device only, and lives on the root element as `data-theme`.
 */
export type Theme = 'auto' | 'light' | 'dark';
export const THEMES: Theme[] = ['auto', 'light', 'dark'];
const KEY = 'phoebe-theme';

function stored(): Theme {
	try {
		const t = localStorage.getItem(KEY);
		return t === 'light' || t === 'dark' ? t : 'auto';
	} catch {
		return 'auto';
	}
}

const initial = stored();
let theme = $state<Theme>(initial);

function apply(t: Theme) {
	if (t === 'auto') delete document.documentElement.dataset.theme;
	else document.documentElement.dataset.theme = t;
}
apply(initial);

export const themes = {
	get theme() {
		return theme;
	},
	set(t: Theme) {
		theme = t;
		apply(t);
		try {
			if (t === 'auto') localStorage.removeItem(KEY);
			else localStorage.setItem(KEY, t);
		} catch {}
	}
};
