import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import type { Plugin } from 'vite';

const ID = 'virtual:pipeline-icons';
const SUFFIX = '-20-filled';

/** The Fluent icons a pipeline can use. The whole collection is 12 MB, so only
 * one style is bundled. */
export function pipelineIcons(): Plugin {
	return {
		name: 'splitwave:pipeline-icons',
		resolveId: (id) => (id === ID ? `\0${ID}` : undefined),
		load(id) {
			if (id !== `\0${ID}`) return;
			const path = createRequire(import.meta.url).resolve('@iconify-json/fluent/icons.json');
			const set = JSON.parse(readFileSync(path, 'utf8'));
			const icons = Object.fromEntries(Object.entries(set.icons).filter(([name]) => name.endsWith(SUFFIX)));
			const subset = { prefix: set.prefix, width: set.width, height: set.height, icons };
			return `export default JSON.parse(${JSON.stringify(JSON.stringify(subset))});`;
		}
	};
}
