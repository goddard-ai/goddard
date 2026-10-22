import assert from 'node:assert/strict'
import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises'
import { createRequire } from 'node:module'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { test } from 'node:test'
import { pathToFileURL } from 'node:url'

const require = createRequire(import.meta.url)
const viteEntry = require.resolve('@nativescript/vite')
const { build } = createRequire(viteEntry)('esbuild')
const { createOptionalDependencyStubEsbuildPlugin } = await import(
	pathToFileURL(join(dirname(viteEntry), 'hmr/shared/vendor/vendor-esbuild-plugins.js'))
)

test('vendor bundling resolves package imports and preserves missing optional dependency guards', async () => {
	const root = await mkdtemp(join(tmpdir(), 'xplat-vendor-imports-'))
	try {
		const dependency = join(root, 'node_modules', 'fixture')
		await mkdir(dependency, { recursive: true })
		await writeFile(join(dependency, 'package.json'), JSON.stringify({
			name: 'fixture',
			type: 'module',
			imports: { '#internal': './internal.js' },
		}))
		await writeFile(join(dependency, 'internal.js'), 'export const value = 42')
		await writeFile(join(dependency, 'index.js'), `
			import { value } from '#internal'
			let optional
			try { optional = require('missing-optional-fixture') } catch { optional = 'fallback' }
			export { value, optional }
		`)
		const result = await build({
			entryPoints: [join(dependency, 'index.js')],
			bundle: true,
			format: 'esm',
			platform: 'neutral',
			write: false,
			plugins: [createOptionalDependencyStubEsbuildPlugin(root)],
		})
		const bundled = await import(`data:text/javascript;base64,${Buffer.from(result.outputFiles[0].text).toString('base64')}`)
		assert.equal(bundled.value, 42)
		assert.equal(bundled.optional, 'fallback')
	} finally {
		await rm(root, { recursive: true, force: true })
	}
})
