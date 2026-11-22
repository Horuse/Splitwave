import type { CompressorData } from '@splitwave/nodes/generated/CompressorData';
import type { DeEsserData } from '@splitwave/nodes/generated/DeEsserData';
import type { DelayData } from '@splitwave/nodes/generated/DelayData';
import type { EqData } from '@splitwave/nodes/generated/EqData';
import type { LimiterData } from '@splitwave/nodes/generated/LimiterData';
import type { NoiseGateData } from '@splitwave/nodes/generated/NoiseGateData';
import type { NoiseSuppressorData } from '@splitwave/nodes/generated/NoiseSuppressorData';
import type { ReverbData } from '@splitwave/nodes/generated/ReverbData';
import type { SaturatorData } from '@splitwave/nodes/generated/SaturatorData';

/** `bypassed` is deliberately absent from every entry: it is per-instance state,
 * and recalling a preset must never silently re-enable an effect the user
 * switched off. Built off the generated types rather than `NodeDataMap`, whose
 * `Record<string, unknown>` intersection erases field types under `Omit`. */
interface PresetDataMap {
	compressor: Omit<CompressorData, 'bypassed'>;
	noiseGate: Omit<NoiseGateData, 'bypassed'>;
	limiter: Omit<LimiterData, 'bypassed'>;
	eq: Omit<EqData, 'bypassed'>;
	reverb: Omit<ReverbData, 'bypassed'>;
	delay: Omit<DelayData, 'bypassed'>;
	saturator: Omit<SaturatorData, 'bypassed'>;
	noiseSuppressor: Omit<NoiseSuppressorData, 'bypassed'>;
	deEsser: Omit<DeEsserData, 'bypassed'>;
	/** Parameter values by id for one plugin, applied live while it runs. */
	plugin: { pluginId: string; params: Record<string, number> };
}

export type PresetKind = keyof PresetDataMap;

export type PresetData<K extends PresetKind = PresetKind> = PresetDataMap[K];

export interface Preset<K extends PresetKind = PresetKind> {
	id: string;
	kind: K;
	name: string;
	data: PresetData<K>;
	createdAt: number;
	/** Schema version; absent means pre-versioning. */
	version?: number;
	/** Ships with the app: selectable, never edited or deleted. */
	builtin?: boolean;
}
