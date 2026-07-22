export type ErrorSource = 'rustPanic' | 'jsError' | 'unhandledRejection';

export interface ErrorEntry {
	source: ErrorSource;
	message: string;
	stack?: string;
	thread?: string;
	at: number;
	/** True when replayed from a crash that killed a previous run, not live. */
	previousRun?: boolean;
}

class ErrorStore {
	current = $state<ErrorEntry | null>(null);

	report(entry: ErrorEntry): void {
		this.current = entry;
	}

	dismiss(): void {
		this.current = null;
	}
}

export const errorStore = new ErrorStore();
