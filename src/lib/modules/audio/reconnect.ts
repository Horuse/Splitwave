export type ReconnectResult = 'unchanged' | 'applied' | 'failed';

function sameIds(left: ReadonlySet<string>, right: ReadonlySet<string>): boolean {
	return left.size === right.size && [...left].every((id) => right.has(id));
}

/** Applies a changed reduced graph. The caller must commit `nextPending` only
 * after this returns `applied`; otherwise a transient failure must be retried. */
export async function reconcilePendingChange<T>(
	currentPending: ReadonlySet<string>,
	nextPending: ReadonlySet<string>,
	reducedGraph: T,
	reconcile: (graph: T) => Promise<void>,
	start: (graph: T) => Promise<void>
): Promise<ReconnectResult> {
	if (sameIds(currentPending, nextPending)) return 'unchanged';
	try {
		await reconcile(reducedGraph);
		return 'applied';
	} catch (error) {
		const message = error instanceof Error ? error.message : String(error);
		if (!message.includes('not running')) return 'failed';
	}
	try {
		await start(reducedGraph);
		return 'applied';
	} catch {
		return 'failed';
	}
}
