import type { NodeCategory } from '$lib/modules/pipeline/types';

/** Data only: the node wrapper and the sidebar both colour by category, and a
 * second copy is how they drifted apart. */
export const CATEGORY_TEXT: Record<NodeCategory, string> = {
	input: 'text-emerald-600 dark:text-emerald-400',
	output: 'text-sky-600 dark:text-sky-400',
	monitor: 'text-amber-600 dark:text-amber-400',
	network: 'text-rose-600 dark:text-rose-400',
	effect: 'text-violet-600 dark:text-violet-400'
};
