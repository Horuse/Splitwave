import { methods } from './methods';
import type { PluginDescriptor } from './types';

/**
 * The installed plugins, shared by every plugin node so a graph with many of
 * them scans once rather than once per node. The backend answers unchanged
 * bundles from its on-disk cache, so a refresh only pays for what changed.
 */
class PluginCatalog {
	plugins = $state<PluginDescriptor[]>([]);
	scanning = $state(false);
	#pending: Promise<void> | null = null;
	#pendingFull = false;

	/** Picks up plugins added, changed or removed since the last scan. */
	refresh(): Promise<void> {
		return this.#scan(false);
	}

	/** Reopens every bundle, including ones that failed to load before. */
	rescan(): Promise<void> {
		return this.#scan(true);
	}

	#scan(full: boolean): Promise<void> {
		// A scan in flight answers anything it covers. A refresh does not cover a
		// rescan, which must reopen bundles the refresh trusted to the cache.
		if (this.#pending) {
			if (!full || this.#pendingFull) return this.#pending;
			return this.#pending.then(() => this.#scan(true));
		}
		this.scanning = true;
		this.#pendingFull = full;
		this.#pending = methods
			.scanPlugins(full)
			.then((found) => {
				this.plugins = found;
			})
			.catch(() => {})
			.finally(() => {
				this.scanning = false;
				this.#pending = null;
			});
		return this.#pending;
	}
}

export const pluginCatalog = new PluginCatalog();
