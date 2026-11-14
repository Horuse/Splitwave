export type LogicOp = 'a' | 'not' | 'and' | 'or' | 'xor' | 'nand' | 'nor' | 'xnor';

export const LOGIC_OPS: { op: LogicOp; label: string; hint: string }[] = [
	{ op: 'a', label: 'A', hint: 'A as it is' },
	{ op: 'not', label: 'NOT', hint: 'A inverted' },
	{ op: 'and', label: 'AND', hint: 'A and B both' },
	{ op: 'or', label: 'OR', hint: 'A or B, or both' },
	{ op: 'xor', label: 'XOR', hint: 'Exactly one of A and B' },
	{ op: 'nand', label: 'NAND', hint: 'Not both A and B' },
	{ op: 'nor', label: 'NOR', hint: 'Neither A nor B' },
	{ op: 'xnor', label: 'XNOR', hint: 'A and B the same' }
];

export function usesB(op: LogicOp): boolean {
	return op !== 'a' && op !== 'not';
}

export function evaluate(op: LogicOp, a: boolean, b: boolean): boolean {
	switch (op) {
		case 'a':
			return a;
		case 'not':
			return !a;
		case 'and':
			return a && b;
		case 'or':
			return a || b;
		case 'xor':
			return a !== b;
		case 'nand':
			return !(a && b);
		case 'nor':
			return !(a || b);
		case 'xnor':
			return a === b;
	}
}
