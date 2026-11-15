import { cva, type VariantProps } from 'class-variance-authority';

export const buttonVariants = cva('btn', {
	variants: {
		tone: {
			neutral: '',
			accent: 'btn-accent',
			success: 'btn-success',
			danger: 'btn-danger',
			warning: 'btn-warning'
		},
		variant: {
			soft: '',
			solid: 'btn-solid',
			ghost: 'btn-ghost'
		},
		size: {
			xs: 'btn-xs',
			sm: 'btn-sm',
			md: 'btn-md',
			lg: 'btn-lg'
		},
		icon: {
			true: 'btn-icon',
			false: ''
		}
	},
	defaultVariants: { tone: 'neutral', variant: 'soft', size: 'md', icon: false }
});

export type ButtonVariants = VariantProps<typeof buttonVariants>;
