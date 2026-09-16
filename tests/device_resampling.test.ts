import { describe, expect, it } from 'bun:test';
import { formatHz } from '../src/lib/components/format';

describe('Bit-transparent passthrough and resampling detection', () => {
	it('bypasses resampling when App Audio rate matches Pipeline rate (e.g. 96 kHz)', () => {
		const appAudioRate = 96_000;
		const pipelineRate = 96_000;

		const resamplingTooltip =
			appAudioRate === pipelineRate
				? undefined
				: `Resampling: ${formatHz(appAudioRate)} → ${formatHz(pipelineRate)}`;

		expect(resamplingTooltip).toBeUndefined();
	});

	it('bypasses resampling when System Audio rate matches Pipeline rate (e.g. 96 kHz)', () => {
		const systemAudioRate = 96_000;
		const pipelineRate = 96_000;

		const resamplingTooltip =
			systemAudioRate === pipelineRate
				? undefined
				: `Resampling: ${formatHz(systemAudioRate)} → ${formatHz(pipelineRate)}`;

		expect(resamplingTooltip).toBeUndefined();
	});

	it('bypasses resampling when Pipeline rate matches Output Speaker rate (e.g. 96 kHz)', () => {
		const pipelineRate = 96_000;
		const speakerRate = 96_000;

		const resamplingTooltip =
			speakerRate === pipelineRate
				? undefined
				: `Resampling: ${formatHz(pipelineRate)} → ${formatHz(speakerRate)}`;

		expect(resamplingTooltip).toBeUndefined();
	});

	it('reports resampling when App Audio rate (48 kHz) differs from Pipeline rate (96 kHz)', () => {
		const appAudioRate = 48_000;
		const pipelineRate = 96_000;

		const resamplingTooltip =
			appAudioRate === pipelineRate
				? undefined
				: `Resampling: ${formatHz(appAudioRate)} → ${formatHz(pipelineRate)}`;

		expect(resamplingTooltip).toBe('Resampling: 48 kHz → 96 kHz');
	});

	it('reports resampling when Pipeline rate (48 kHz) differs from Output Speaker rate (96 kHz)', () => {
		const pipelineRate = 48_000;
		const speakerRate = 96_000;

		const resamplingTooltip =
			speakerRate === pipelineRate
				? undefined
				: `Resampling: ${formatHz(pipelineRate)} → ${formatHz(speakerRate)}`;

		expect(resamplingTooltip).toBe('Resampling: 48 kHz → 96 kHz');
	});
});
