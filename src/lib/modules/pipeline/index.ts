import { methods } from './methods';
import { pipelineStore } from './stores.svelte';
import type * as types from '@splitwave/nodes';

const Pipeline = {
	methods,
	stores: { pipeline: pipelineStore }
};

export type { types };
export default Pipeline;
