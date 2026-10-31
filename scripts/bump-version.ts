const SEMVER = /^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/;

const version = process.argv[2];
if (!version || !SEMVER.test(version)) {
	console.error('usage: bun run bump <version>   e.g. 1.5.1 or 2.0.0-beta.1');
	process.exit(1);
}
const prerelease = version.includes('-');
const date = new Date().toLocaleDateString('sv-SE');

async function edit(path: string, pattern: RegExp, replacement: string) {
	const text = await Bun.file(path).text();
	if (!pattern.test(text)) {
		console.error(`${path}: pattern ${pattern} not found`);
		process.exit(1);
	}
	await Bun.write(path, text.replace(pattern, replacement));
	console.log(`updated ${path}`);
}

await edit('package.json', /("version":\s*")[^"]+(")/, `$1${version}$2`);
await edit('src-tauri/tauri.conf.json', /("version":\s*")[^"]+(")/, `$1${version}$2`);
await edit('src-tauri/Cargo.toml', /(\[package\][^[]*?\nversion = ")[^"]+(")/, `$1${version}$2`);
await edit('src-tauri/Cargo.lock', /(name = "splitwave"\nversion = ")[^"]+(")/, `$1${version}$2`);
await edit('CHANGELOG.md', /(\n## \[)/, `\n## [${version}] - ${date}\n$1`);

// AppStream sorts "2.0.0-beta.1" above "2.0.0", so pre-releases stay out of the Flathub metadata.
if (!prerelease) {
	await edit(
		'flatpak/com.horuse.splitwave.metainfo.xml',
		/(<releases>\n)/,
		`$1    <release version="${version}" date="${date}" />\n`
	);
}
