import { beforeEach, describe, expect, it, mock } from 'bun:test';

type Invoke = (command: string, args?: Record<string, unknown>) => Promise<unknown>;
type DownloadHandler = (emit: (event: UpdateEvent) => void) => Promise<void>;
type UpdateEvent =
	| { event: 'Started'; data: { contentLength?: number } }
	| { event: 'Progress'; data: { chunkLength: number } }
	| { event: 'Finished'; data: Record<string, never> };

let invokeImpl: Invoke = async () => null;
let downloadHandler: DownloadHandler = async () => {};
let relaunches = 0;
const persisted = new Map<string, unknown>();
const appSettings = { includePreReleases: false, maxSnapshots: 20 };
const updaterStore: { state: any } = { state: { phase: 'idle' } };

class MockUpdate {
	version: string;
	body?: string;

	constructor(metadata: { version: string; body?: string }) {
		this.version = metadata.version;
		this.body = metadata.body;
	}

	async downloadAndInstall(emit: (event: UpdateEvent) => void): Promise<void> {
		await downloadHandler(emit);
	}
}

mock.module('@tauri-apps/plugin-updater', () => ({ Update: MockUpdate }));
mock.module('@tauri-apps/plugin-process', () => ({
	relaunch: async () => {
		relaunches += 1;
	}
}));
mock.module('@tauri-apps/plugin-store', () => ({
	LazyStore: class {
		async get<T>(key: string): Promise<T | undefined> {
			return persisted.get(key) as T | undefined;
		}
		async set(key: string, value: unknown): Promise<void> {
			persisted.set(key, value);
		}
		async save(): Promise<void> {}
	}
}));
mock.module('@tauri-apps/api/core', () => ({
	invoke: (command: string, args?: Record<string, unknown>) => invokeImpl(command, args)
}));
mock.module('@tauri-apps/plugin-os', () => ({ arch: () => 'arm64', type: () => 'macos' }));
mock.module('$lib/modules/settings/stores.svelte', () => ({ appSettings }));
mock.module('../src/lib/modules/updater/stores.svelte.ts', () => ({ updaterStore }));

const { checkForUpdates, getSkippedVersion, installUpdate, latestRelease, skipVersion } = await import('../src/lib/modules/updater/methods');

function metadata(version = '2.0.0', body = 'Bundled notes') {
	return {
		rid: 7,
		currentVersion: '1.0.0',
		version,
		body,
		rawJson: { version }
	};
}

beforeEach(() => {
	invokeImpl = async () => null;
	downloadHandler = async () => {};
	relaunches = 0;
	persisted.clear();
	appSettings.includePreReleases = false;
	updaterStore.state = { phase: 'idle' };
	globalThis.fetch = (async () => new Response(null, { status: 404 })) as unknown as typeof fetch;
});

describe('update checks', () => {
	it('reports no update only for an explicit manual check', async () => {
		await checkForUpdates(false);
		expect(updaterStore.state).toEqual({ phase: 'up_to_date' });

		await checkForUpdates(true);
		expect(updaterStore.state).toEqual({ phase: 'idle' });
	});

	it('honors a skipped version only during silent startup checks', async () => {
		await skipVersion('2.0.0');
		expect(await getSkippedVersion()).toBe('2.0.0');
		invokeImpl = async () => metadata();

		await checkForUpdates(true);
		expect(updaterStore.state).toEqual({ phase: 'idle' });

		await checkForUpdates(false);
		expect(updaterStore.state.phase).toBe('available');
		expect(updaterStore.state.update.version).toBe('2.0.0');
		expect(updaterStore.state.notes).toBe('Bundled notes');
	});

	it('selects a non-draft prerelease manifest and release notes', async () => {
		appSettings.includePreReleases = true;
		const requests: string[] = [];
		globalThis.fetch = (async (input) => {
			const url = String(input);
			requests.push(url);
			if (url.endsWith('/releases')) {
				return Response.json([
					{ draft: true, assets: [{ name: 'latest.json', browser_download_url: 'https://bad.test/latest.json' }] },
					{ draft: false, assets: [{ name: 'latest.json', browser_download_url: 'https://good.test/latest.json' }] }
				]);
			}
			return Response.json({ tag_name: 'v2.0.0-rc.1', body: '  Remote notes  ' });
		}) as typeof fetch;
		let endpoint: unknown;
		invokeImpl = async (command, args) => {
			expect(command).toBe('check_for_updates');
			endpoint = args?.endpoint;
			return metadata('2.0.0-rc.1');
		};

		await checkForUpdates(false);
		expect(endpoint).toBe('https://good.test/latest.json');
		expect(requests).toEqual([
			'https://api.github.com/repos/Horuse/Splitwave/releases',
			'https://api.github.com/repos/Horuse/Splitwave/releases/tags/v2.0.0-rc.1'
		]);
		expect(updaterStore.state.notes).toBe('Remote notes');
	});

	it('distinguishes unsupported builds from diagnostic failures', async () => {
		invokeImpl = async () => {
			throw new Error('None of the fallback platforms matched');
		};
		await checkForUpdates(false);
		expect(updaterStore.state.phase).toBe('unsupported');
		expect(updaterStore.state.message).toContain('arm64');
		expect(updaterStore.state.message).toContain('macOS');

		invokeImpl = async (command) => {
			if (command === 'diagnose_update_error') return 'certificate chain rejected';
			throw new Error('network request failed');
		};
		await checkForUpdates(false);
		expect(updaterStore.state).toEqual({
			phase: 'error',
			message: 'network request failed\n\ncertificate chain rejected'
		});
	});

	it('normalizes latest release metadata and handles unavailable GitHub', async () => {
		globalThis.fetch = (async () => Response.json({ tag_name: 'v3.1.0', body: '  Notes  ' })) as unknown as typeof fetch;
		expect(await latestRelease()).toEqual({ version: '3.1.0', notes: 'Notes' });

		globalThis.fetch = (async () => {
			throw new Error('offline');
		}) as unknown as typeof fetch;
		expect(await latestRelease()).toBeNull();
	});
});

describe('update installation', () => {
	it('tracks download progress and relaunches after installation', async () => {
		const update = new MockUpdate(metadata());
		updaterStore.state = { phase: 'available', update, notes: null };
		downloadHandler = async (emit) => {
			emit({ event: 'Started', data: { contentLength: 100 } });
			emit({ event: 'Progress', data: { chunkLength: 35 } });
			emit({ event: 'Progress', data: { chunkLength: 65 } });
			expect(updaterStore.state).toMatchObject({ phase: 'downloading', downloaded: 100, total: 100 });
			emit({ event: 'Finished', data: {} });
		};

		await installUpdate();
		expect(updaterStore.state.phase).toBe('installing');
		expect(relaunches).toBe(1);
	});

	it('surfaces installation failures without relaunching', async () => {
		const update = new MockUpdate(metadata());
		updaterStore.state = { phase: 'available', update, notes: null };
		downloadHandler = async () => {
			throw new Error('signature verification failed');
		};

		await installUpdate();
		expect(updaterStore.state).toEqual({ phase: 'error', message: 'signature verification failed' });
		expect(relaunches).toBe(0);
	});

	it('does nothing unless an update is available', async () => {
		updaterStore.state = { phase: 'idle' };
		await installUpdate();
		expect(updaterStore.state).toEqual({ phase: 'idle' });
		expect(relaunches).toBe(0);
	});
});
