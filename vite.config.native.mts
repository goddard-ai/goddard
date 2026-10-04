import { fileURLToPath } from 'node:url'
import { defineConfig } from 'vite'
import { xplatNative } from '@octane-xplat/cli/vite'

// Keep these packages out of the dev vendor bundle (the HMR-mode esbuild
// pass that runs before the dev server starts). The vendor pipeline cannot
// build them, and two of them must never be bundled anyway:
//   - @octane-xplat/platform ships `src/*.ts` that import `.tsrx` files —
//     the vendor esbuild has no `.tsrx` loader. Per-module /ns/m serving
//     handles them correctly.
//   - @octane-xplat/effects exports only `./ios`/`./android` subpaths — the
//     bare-root vendor entry cannot resolve it.
//   - @xterm/* reference DOM globals at module eval; vendoring them aborts
//     boot with `document is not defined`. App code reaches them only
//     through `.web` leaves (see lib/terminal-assets).
process.env.NS_VENDOR_EXCLUDE = [
	'@octane-xplat/platform',
	'@octane-xplat/effects',
	'@xterm/xterm',
	'@xterm/addon-fit',
	...(process.env.NS_VENDOR_EXCLUDE?.split(',') ?? []),
].join(',')

// The shared preset owns the renderer rules, octane→universal/native alias,
// suffix extension chain, deps-bundle plugin exclusions, the HMR watchdog,
// and the px→dip CSS rewrite. App-specific additions go through the second
// argument's `extra` (or mergeConfig).
export default defineConfig(({ mode }) =>
	xplatNative(mode, {
		extra: {
			resolve: {
				alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) },
			},
		},
	}),
)
