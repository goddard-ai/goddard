import { existsSync, symlinkSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const projectRoot = resolve(fileURLToPath(new URL('..', import.meta.url)));
const allocLink = resolve(projectRoot, '..', 'alloc');

if (existsSync(allocLink)) {
	process.exit(0);
}

const fallback = '/Users/alec/dev/alloc';
if (!existsSync(fallback)) {
	console.error(`Cannot find ../alloc or fallback checkout at ${fallback}`);
	process.exit(1);
}

symlinkSync(fallback, allocLink, 'dir');
console.log(`Linked ${allocLink} -> ${fallback}`);
