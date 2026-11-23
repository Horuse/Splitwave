export interface NavNode {
	id: string;
	label: string;
	href?: string;
	children?: NavNode[];
}
