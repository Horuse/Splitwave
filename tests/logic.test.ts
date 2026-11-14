import { describe, expect, it } from 'bun:test';
import { evaluate, LOGIC_OPS, usesB, type LogicOp } from '../src/lib/modules/flow/logic';

const TRUTH: Record<LogicOp, [boolean, boolean, boolean, boolean]> = {
	// Outputs for (a, b) = 00, 01, 10, 11.
	a: [false, false, true, true],
	not: [true, true, false, false],
	and: [false, false, false, true],
	or: [false, true, true, true],
	xor: [false, true, true, false],
	nand: [true, true, true, false],
	nor: [true, false, false, false],
	xnor: [true, false, false, true]
};

describe('evaluate', () => {
	it('matches the truth table of every operator', () => {
		for (const { op } of LOGIC_OPS) {
			const out = [
				[false, false],
				[false, true],
				[true, false],
				[true, true]
			].map(([a, b]) => evaluate(op, a, b));
			expect(out).toEqual(TRUTH[op]);
		}
	});
});

describe('usesB', () => {
	it('is false only for the one-input operators', () => {
		expect(LOGIC_OPS.filter(({ op }) => !usesB(op)).map(({ op }) => op)).toEqual(['a', 'not']);
	});
});
