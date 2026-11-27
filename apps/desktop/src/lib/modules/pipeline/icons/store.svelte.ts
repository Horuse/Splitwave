import { getIconData, iconToSVG } from '@iconify/utils';

export type IconSvg = { viewBox: string; body: string };
type IconSet = Parameters<typeof getIconData>[0];

class IconStore {
	private set = $state.raw<IconSet | null>(null);
	private loading: Promise<void> | null = null;

	/** Empty until the set has loaded. */
	get names(): string[] {
		return this.set ? Object.keys(this.set.icons) : [];
	}

	load(): Promise<void> {
		this.loading ??= import('virtual:pipeline-icons').then((module) => {
			this.set = module.default;
		});
		return this.loading;
	}

	/** Markup comes only from the bundled set: stored data holds just the name. */
	svg(name: string): IconSvg | null {
		const data = this.set ? getIconData(this.set, name) : null;
		if (!data) return null;
		const { attributes, body } = iconToSVG(data);
		return { viewBox: attributes.viewBox, body };
	}
}

export const iconStore = new IconStore();
