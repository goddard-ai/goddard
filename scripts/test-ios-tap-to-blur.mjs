import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import test from 'node:test'
import vm from 'node:vm'
import ts from 'typescript'

test('packaged iOS tap-to-blur implements every exposed selector', async () => {
	const source = await readFile(new URL('../node_modules/@octane-xplat/ui/dist/native/tap-to-blur.ios.js', import.meta.url), 'utf8')
	const compiled = ts.transpile(source, { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 })
	const recognizers = []
	let endedEditing = false
	const window = {
		addGestureRecognizer: recognizer => recognizers.push(recognizer),
		removeGestureRecognizer: recognizer => assert.equal(recognizer, recognizers[0]),
		endEditing: force => { endedEditing = force },
	}
	const context = vm.createContext({
		exports: {},
		NSObject: {
			extend(methods, options) {
				for (const selector of Object.keys(options.exposedMethods)) {
					assert.equal(typeof methods[selector], 'function', `${selector} must have an implementation`)
				}
				return { new: () => Object.create(methods) }
			},
		},
		UIGestureRecognizerDelegate: {},
		interop: { types: { void: {}, id: {} } },
		UITapGestureRecognizer: {
			alloc: () => ({ initWithTargetAction: (target, action) => ({ target, action }) }),
		},
		UIApplication: { sharedApplication: { connectedScenes: { allObjects: [] }, keyWindow: window } },
	})
	vm.runInContext(compiled, context)
	const detach = context.exports.attachTapToBlur({})
	const recognizer = recognizers[0]
	assert.equal(recognizer.cancelsTouchesInView, false)
	recognizer.target[recognizer.action](recognizer)
	assert.equal(endedEditing, true)
	detach()
})
