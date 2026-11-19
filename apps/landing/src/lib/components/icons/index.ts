import type { Component } from 'svelte';
import type { SVGAttributes } from 'svelte/elements';

/** Shape every icon here shares (unplugin-icons components), so callers can keep one in a data table. */
export type IconComponent = Component<SVGAttributes<SVGSVGElement>>;

// Fluent, the app's icon set: monochrome, inherits currentColor.
export { default as ArrowUpRight } from '~icons/fluent/arrow-up-right-16-filled';
export { default as Desktop } from '~icons/fluent/desktop-24-regular';
export { default as Laptop } from '~icons/fluent/laptop-24-regular';
export { default as HardDrive } from '~icons/fluent/hard-drive-24-regular';
export { default as Box } from '~icons/fluent/box-24-regular';
export { default as ArrowDownload } from '~icons/fluent/arrow-download-24-filled';
export { default as ArrowLeft } from '~icons/fluent/arrow-left-24-regular';
export { default as Copy } from '~icons/fluent/copy-24-regular';
export { default as Checkmark } from '~icons/fluent/checkmark-24-regular';
export { default as CheckmarkCircle } from '~icons/fluent/checkmark-circle-24-filled';
export { default as ShieldCheckmark } from '~icons/fluent/shield-checkmark-24-regular';
export { default as Console } from '~icons/fluent/window-console-20-regular';
export { default as Warning } from '~icons/fluent/warning-24-regular';
export { default as Info } from '~icons/fluent/info-24-regular';
export { default as Heart } from '~icons/fluent/heart-24-filled';
export { default as Tag } from '~icons/fluent/tag-24-regular';
export { default as Open } from '~icons/fluent/open-24-regular';

// Brand marks, monochrome: pass the brand colour as a text-* class at the call site.
export { default as Github } from '~icons/simple-icons/github';
export { default as Patreon } from '~icons/simple-icons/patreon';
export { default as Apple } from '~icons/simple-icons/apple';
export { default as Linux } from '~icons/simple-icons/linux';
export { default as Debian } from '~icons/simple-icons/debian';
export { default as Fedora } from '~icons/simple-icons/fedora';
export { default as Ubuntu } from '~icons/simple-icons/ubuntu';
export { default as RedHat } from '~icons/simple-icons/redhat';
export { default as OpenSuse } from '~icons/simple-icons/opensuse';
export { default as AppImage } from '~icons/simple-icons/appimage';

// Marks that carry their own colours.
export { default as Windows } from '~icons/logos/microsoft-windows-icon';
export { default as Bitcoin } from '~icons/token-branded/btc';
export { default as Tether } from '~icons/token-branded/usdt';
export { default as Ton } from '~icons/token-branded/ton';
export { default as Tron } from '~icons/token-branded/trx';
