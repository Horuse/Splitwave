import { createId } from '@paralleldrive/cuid2';
import type { Component } from 'svelte';

export interface ModalBaseProps {
	modalId: string;
}

export type ModalComponent<Props = Record<string, unknown>> = Component<ModalBaseProps & Props>;

export interface ModalParams {
	canClose?: boolean;
	[key: string]: unknown;
}

export interface ModalEntry {
	id: string;
	title: string;
	component: ModalComponent;
	params: ModalParams;
	zIndex: number;
}

interface Resolvers {
	resolve: (value: unknown) => void;
}

class ModalManager {
	private _modals = $state<ModalEntry[]>([]);
	private resolvers = new Map<string, Resolvers>();
	private nextZ = 100;
	private escListener: ((e: KeyboardEvent) => void) | undefined;

	get modals(): readonly ModalEntry[] {
		return this._modals;
	}

	get isOpen(): boolean {
		return this._modals.length > 0;
	}

	open<Result = unknown, P extends ModalParams = ModalParams>(
		title: string,
		component: ModalComponent<P>,
		params: P = {} as P
	): Promise<Result | undefined> {
		this.ensureEsc();
		return new Promise((resolve) => {
			const id = createId();
			this.resolvers.set(id, { resolve: resolve as (v: unknown) => void });
			this._modals = [
				...this._modals,
				{
					id,
					title,
					component: component as ModalComponent,
					params: { canClose: true, ...params },
					zIndex: this.nextZ++
				}
			];
			this.applyBodyLock();
		});
	}

	close(id: string, result?: unknown): void {
		const r = this.resolvers.get(id);
		this._modals = this._modals.filter((m) => m.id !== id);
		if (r) {
			r.resolve(result);
			this.resolvers.delete(id);
		}
		this.applyBodyLock();
	}

	closeTop(result?: unknown): void {
		if (this._modals.length === 0) return;
		const top = this._modals[this._modals.length - 1];
		if (top.params.canClose === false) return;
		this.close(top.id, result);
	}

	closeAll(): void {
		const ids = this._modals.map((m) => m.id);
		ids.forEach((id) => this.close(id));
	}

	private ensureEsc(): void {
		if (this.escListener || typeof window === 'undefined') return;
		this.escListener = (e: KeyboardEvent) => {
			if (e.key === 'Escape') this.closeTop();
		};
		window.addEventListener('keydown', this.escListener);
	}

	private applyBodyLock(): void {
		if (typeof document === 'undefined') return;
		document.body.classList.toggle('overflow-hidden', this._modals.length > 0);
	}
}

export const modalManager = new ModalManager();
