/// <reference path="../worker-configuration.d.ts" />

declare global {
	namespace App {
		interface Platform {
			env: Record<string, never>;
		}
	}
}

export {};
