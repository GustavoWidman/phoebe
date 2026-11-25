import { resolve } from 'node:path';
import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { SERVICE_WORKER, serviceWorker } from './scripts/service-worker.ts';

export default defineConfig({
	publicDir: 'static',
	// `phoebe web --fixture` serves a simulated fleet here
	server: {
		proxy: { '/api': 'http://127.0.0.1:7980', '/fixture': 'http://127.0.0.1:7980' }
	},
	build: {
		outDir: 'build',
		// iOS 18 and the desktop browsers of its time draw every color the system uses as
		// written, so the minifier lowers none of them
		cssTarget: ['safari18', 'chrome130', 'firefox132'],
		// Every name under this path carries a content hash.
		assetsDir: '_app/immutable',
		// Every browser the app supports preloads modules itself.
		modulePreload: { polyfill: false },
		// what each route loads, for scripts/budget.ts
		manifest: true,
		rollupOptions: {
			input: {
				index: resolve(import.meta.dirname, 'index.html'),
				'service-worker': resolve(import.meta.dirname, 'src/service-worker/index.ts')
			},
			output: {
				// the service worker keeps one name, which browsers check for a new build
				entryFileNames: (chunk) =>
					chunk.name === 'service-worker' ? SERVICE_WORKER : '_app/immutable/[name]-[hash].js'
			}
		}
	},
	plugins: [svelte({ compilerOptions: { runes: true } }), serviceWorker()],
	test: {
		include: ['src/**/*.test.ts', 'scripts/**/*.test.ts'],
		environment: 'node',
		expect: { requireAssertions: true }
	}
});
