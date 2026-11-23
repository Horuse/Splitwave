// Article typography for mdsvex output, shared by the app and the landing.
// Embedded app UI, such as a pipeline diagram, opts out with .not-prose.
// Written out in full so Tailwind finds every class when it scans this file.
export const PROSE = [
	'[&_h2:not(.not-prose_*)]:mt-10 [&_h2:not(.not-prose_*)]:scroll-mt-6 [&_h2:not(.not-prose_*)]:text-lg [&_h2:not(.not-prose_*)]:font-semibold [&_h2:not(.not-prose_*)]:text-theme',
	'[&_h3:not(.not-prose_*)]:mt-7 [&_h3:not(.not-prose_*)]:scroll-mt-6 [&_h3:not(.not-prose_*)]:font-semibold [&_h3:not(.not-prose_*)]:text-theme',
	'[&_p:not(.not-prose_*)]:mt-3 [&_ul:not(.not-prose_*)]:mt-3 [&_ul:not(.not-prose_*)]:list-disc [&_ul:not(.not-prose_*)]:pl-5 [&_ol:not(.not-prose_*)]:mt-3 [&_ol:not(.not-prose_*)]:list-decimal [&_ol:not(.not-prose_*)]:pl-5 [&_li:not(.not-prose_*)]:mt-1 [&_li:not(.not-prose_*)]:pl-1',
	'[&_a:not(.not-prose_*)]:text-blue-500 [&_a:not(.not-prose_*)]:underline [&_a:not(.not-prose_*)]:underline-offset-2',
	'[&_strong:not(.not-prose_*)]:font-semibold [&_strong:not(.not-prose_*)]:text-theme',
	'[&_code:not(.not-prose_*)]:rounded [&_code:not(.not-prose_*)]:bg-neutral-200 [&_code:not(.not-prose_*)]:px-1 [&_code:not(.not-prose_*)]:py-px [&_code:not(.not-prose_*)]:font-mono [&_code:not(.not-prose_*)]:text-[0.88em]',
	'[&_pre:not(.not-prose_*)]:mt-4 [&_pre:not(.not-prose_*)]:overflow-x-auto [&_pre:not(.not-prose_*)]:rounded-xl [&_pre:not(.not-prose_*)]:border [&_pre:not(.not-prose_*)]:border-neutral-300 [&_pre:not(.not-prose_*)]:bg-neutral-100 [&_pre:not(.not-prose_*)]:p-4 [&_pre_code:not(.not-prose_*)]:bg-transparent [&_pre_code:not(.not-prose_*)]:p-0',
	'[&_blockquote:not(.not-prose_*)]:mt-4 [&_blockquote:not(.not-prose_*)]:rounded-r-lg [&_blockquote:not(.not-prose_*)]:border-l-2 [&_blockquote:not(.not-prose_*)]:border-neutral-500 [&_blockquote:not(.not-prose_*)]:bg-neutral-100 [&_blockquote:not(.not-prose_*)]:px-4 [&_blockquote:not(.not-prose_*)]:py-2 [&_blockquote_p:first-child:not(.not-prose_*)]:mt-0',
	'[&_table:not(.not-prose_*)]:mt-4 [&_table:not(.not-prose_*)]:w-full [&_table:not(.not-prose_*)]:border-collapse [&_table:not(.not-prose_*)]:overflow-hidden [&_table:not(.not-prose_*)]:rounded-xl [&_table:not(.not-prose_*)]:text-[0.93em]',
	'[&_th:not(.not-prose_*)]:border [&_th:not(.not-prose_*)]:border-neutral-300 [&_th:not(.not-prose_*)]:bg-neutral-100 [&_th:not(.not-prose_*)]:px-3 [&_th:not(.not-prose_*)]:py-2 [&_th:not(.not-prose_*)]:text-left [&_th:not(.not-prose_*)]:font-semibold [&_th:not(.not-prose_*)]:text-theme',
	'[&_td:not(.not-prose_*)]:border [&_td:not(.not-prose_*)]:border-neutral-300 [&_td:not(.not-prose_*)]:px-3 [&_td:not(.not-prose_*)]:py-2 [&_td:not(.not-prose_*)]:align-top',
	'[&_kbd:not(.not-prose_*)]:rounded [&_kbd:not(.not-prose_*)]:border [&_kbd:not(.not-prose_*)]:border-neutral-400 [&_kbd:not(.not-prose_*)]:bg-neutral-100 [&_kbd:not(.not-prose_*)]:px-1.5 [&_kbd:not(.not-prose_*)]:font-sans [&_kbd:not(.not-prose_*)]:text-xs',
	'[&_img:not(.not-prose_*)]:mt-4 [&_img:not(.not-prose_*)]:rounded-xl [&_img:not(.not-prose_*)]:border [&_img:not(.not-prose_*)]:border-neutral-300'
].join(' ');
