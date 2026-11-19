import { mkdir, readdir, unlink } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import puppeteer from 'puppeteer';

const URL = process.env.PREVIEW_URL ?? 'http://localhost:1420/preview';
const OUT = join(dirname(fileURLToPath(import.meta.url)), '..', 'static', 'nodes');
const SCALE = 2;

const snake = (kind) => kind.replace(/[A-Z]/g, (c) => `_${c.toLowerCase()}`);

const browser = await puppeteer.launch({ headless: 'new' });
const page = await browser.newPage();
await page.setViewport({ width: 1600, height: 1200, deviceScaleFactor: SCALE });

try {
	await page.goto(URL, { waitUntil: 'networkidle0', timeout: 30_000 });
} catch (err) {
	await browser.close();
	throw new Error(`Cannot reach ${URL}. Start the BetterAudio dev server first.`, { cause: err });
}

await page.waitForSelector('[data-node-kind]');
// omitBackground only yields alpha if every ancestor of the node is transparent.
await page.addStyleTag({
	content: `html, body, body > div, [data-node-kind], [data-node-kind] > * > section,
	          section, section > div { background: transparent !important; }`
});
// Canvas-backed nodes (waveform, meters) need a frame to paint before capture.
await new Promise((r) => setTimeout(r, 1500));

const kinds = await page.$$eval('[data-node-kind]', (els) => els.map((el) => el.dataset.nodeKind));

await mkdir(OUT, { recursive: true });
const existing = new Set(await readdir(OUT));
const written = new Set();

for (const kind of kinds) {
	const el = await page.$(`[data-node-kind="${kind}"] > *`);
	const box = await el.boundingBox();
	if (!box) throw new Error(`Node "${kind}" has no layout box`);

	const file = `${snake(kind)}.webp`;
	await page.screenshot({
		path: join(OUT, file),
		type: 'webp',
		quality: 92,
		captureBeyondViewport: true,
		omitBackground: true,
		clip: { x: box.x, y: box.y, width: box.width, height: box.height, scale: SCALE }
	});
	written.add(file);
	console.log(`${file}  ${Math.round(box.width)}x${Math.round(box.height)}`);
}

for (const file of existing) {
	if (file.endsWith('.webp') && !written.has(file)) {
		await unlink(join(OUT, file));
		console.log(`removed stale ${file}`);
	}
}

await browser.close();
console.log(`\n${written.size} nodes -> static/nodes`);
