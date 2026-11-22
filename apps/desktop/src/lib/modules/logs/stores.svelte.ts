import { methods } from './methods';
import type { LogEntry } from './types';

const CAPACITY = 2000;
const CONSOLE_LEVELS = {
	debug: 'DEBUG',
	log: 'INFO',
	info: 'INFO',
	warn: 'WARN',
	error: 'ERROR'
} as const;

function stringify(args: unknown[]): string {
	return args
		.map((a) => {
			if (typeof a === 'string') return a;
			if (a instanceof Error) return `${a.name}: ${a.message}`;
			try {
				return JSON.stringify(a);
			} catch {
				return String(a);
			}
		})
		.join(' ');
}

class LogStore {
	open = $state(false);
	js = $state<LogEntry[]>([]);
	rust = $state<LogEntry[]>([]);

	private installed = false;

	get entries(): LogEntry[] {
		return [...this.rust, ...this.js].sort((a, b) => a.at - b.at);
	}

	/** Console output is only visible in a devtools window, which release builds
	 * have no way to open -- mirror it so the in-app viewer can show it. */
	installConsoleCapture(): void {
		if (this.installed) return;
		this.installed = true;
		for (const [name, level] of Object.entries(CONSOLE_LEVELS)) {
			const key = name as keyof typeof CONSOLE_LEVELS;
			const original = console[key].bind(console);
			console[key] = (...args: unknown[]) => {
				original(...args);
				// Console calls can land mid-render (e.g. xyflow's internal warnings
				// fire from inside a $derived) -- state writes there are forbidden,
				// so push after the current task instead of inline.
				setTimeout(() => this.push(level, stringify(args)), 0);
			};
		}
	}

	push(level: string, message: string, target = 'webview'): void {
		const next = [...this.js, { at: Date.now(), level, target, message, origin: 'js' as const }];
		this.js = next.length > CAPACITY ? next.slice(next.length - CAPACITY) : next;
	}

	async refresh(): Promise<void> {
		const lines = await methods.getLogs();
		this.rust = lines.map((l) => ({ ...l, origin: 'rust' as const }));
	}

	async clear(): Promise<void> {
		this.js = [];
		this.rust = [];
		await methods.clearLogs();
	}
}

export const logStore = new LogStore();
