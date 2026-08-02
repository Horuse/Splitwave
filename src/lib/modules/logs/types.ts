export type LogOrigin = 'rust' | 'js';

export interface LogLine {
	at: number;
	level: string;
	target: string;
	message: string;
}

export interface LogEntry extends LogLine {
	origin: LogOrigin;
}
