/**
 * The build's half of the service worker: the list of every file the build wrote, spliced into
 * service-worker.js with a version that changes whenever any of them does.
 */
import { createHash } from 'node:crypto';
import { readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';
import type { Plugin } from 'vite';

function files(dir: string): string[] {
	return readdirSync(dir).flatMap((name) => {
		const path = join(dir, name);
		return statSync(path).isDirectory() ? files(path) : [path];
	});
}

export const SERVICE_WORKER = 'service-worker.js';

/** tells the service worker every file the build made */
export function serviceWorker(): Plugin {
	let out = '';
	return {
		name: 'phoebe-service-worker',
		apply: 'build',
		configResolved(config) {
			out = resolve(config.root, config.build.outDir);
		},
		closeBundle() {
			const version = createHash('sha256');
			const precache: string[] = [];
			for (const path of files(out).sort()) {
				const name = relative(out, path).split('\\').join('/');
				if (name === SERVICE_WORKER || name.startsWith('.vite/')) continue;
				version.update(name).update(readFileSync(path));
				// the shell is cached as `/`, which is how every navigation asks for it
				if (name !== 'index.html') precache.push(`/${name}`);
			}
			const sw = join(out, SERVICE_WORKER);
			const source = readFileSync(sw, 'utf8')
				.replace(/(["'`])__PHOEBE_BUILD__\1/, JSON.stringify(version.digest('hex').slice(0, 12)))
				.replace(/(["'`])__PHOEBE_FILES__\1/, JSON.stringify(JSON.stringify(precache)));
			if (source.includes('__PHOEBE_')) throw new Error(`${SERVICE_WORKER}: a placeholder is left`);
			// it registers as a classic script, which cannot load a chunk it shares with the app
			if (/(?:^|[;}\s])import\s*[{*"']/.test(source))
				throw new Error(`${SERVICE_WORKER}: imports a module the app also imports`);
			writeFileSync(sw, source);
		}
	};
}
