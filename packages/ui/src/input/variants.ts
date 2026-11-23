import { cva, type VariantProps } from 'class-variance-authority';

export const inputVariants = cva('input', {
	variants: {
		size: { sm: 'input-sm', base: '', md: 'input-md' },
		mono: { true: 'input-mono', false: '' }
	},
	defaultVariants: { size: 'base', mono: false }
});

export type InputVariants = VariantProps<typeof inputVariants>;
