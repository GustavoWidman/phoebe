/** Desktop from 900px up: three columns, and tables for long lists. Below it, the phone. */
const query = matchMedia('(min-width: 900px)');
let desk = $state(query.matches);
query.addEventListener('change', (e) => (desk = e.matches));

export const layout = {
	get desk() {
		return desk;
	}
};
