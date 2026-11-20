/** A `<script type="application/ld+json">` tag for `{@html}`. Data can carry text
 * from outside (a release tag, an article title), so `<`, `>` and `&` are escaped:
 * a value containing `</script>` cannot close the tag and run as markup. */
export function jsonLdTag(data: unknown): string {
	const json = JSON.stringify(data).replace(/</g, '\\u003c').replace(/>/g, '\\u003e').replace(/&/g, '\\u0026');
	return `<script type="application/ld+json">${json}</script>`;
}
