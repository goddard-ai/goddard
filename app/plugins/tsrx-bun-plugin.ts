import { readFile } from "node:fs/promises"
import { compile } from "@tsrx/preact"
import type { BunPlugin } from "bun"

const DEFAULT_INCLUDE = /\.tsrx$/
const CSS_QUERY = "?tsrx-css&lang.css"
const CSS_QUERY_PATTERN = /\?tsrx-css&lang\.css$/

export interface Options {
  include?: RegExp
  exclude?: RegExp | RegExp[]
  jsxImportSource?: string
  suspenseSource?: string
  emitCss?: boolean
}

function test_pattern(pattern: RegExp, value: string): boolean {
  pattern.lastIndex = 0
  return pattern.test(value)
}

function matches_pattern(pattern: RegExp | RegExp[] | undefined, value: string): boolean {
  if (!pattern) return false
  if (Array.isArray(pattern)) {
    return pattern.some((entry) => test_pattern(entry, value))
  }
  return test_pattern(pattern, value)
}

function should_compile(options: Options, value: string): boolean {
  const include = options.include ?? DEFAULT_INCLUDE
  return test_pattern(include, value) && !matches_pattern(options.exclude, value)
}

function to_css_id(file_path: string): string {
  return file_path + CSS_QUERY
}

/**
 * Bun plugin for `.tsrx` files that compiles them through `@tsrx/preact` and
 * then runs Bun's TSX transform so the final output calls Preact's automatic
 * JSX runtime. Component-local styles are exposed as virtual CSS modules.
 */
export function tsrxPreact(options: Options = {}) {
  const jsx_import_source = options.jsxImportSource ?? "preact"
  const emit_css = options.emitCss ?? true
  const compile_options = {
    suspenseSource: options.suspenseSource,
  }

  const css_cache = new Map<string, string>()

  return {
    name: "@tsrx/bun-plugin-preact",
    setup(build) {
      // build.config is only present for Bun.build(); runtime registration
      // via Bun.plugin(), including bun:test preloads, does not provide it.
      const build_config = build.config ?? {}
      const transpiler = new Bun.Transpiler({
        loader: "tsx",
        target: build_config.target,
        autoImportJSX: true,
        tsconfig: {
          compilerOptions: {
            jsx: "react-jsx",
            jsxImportSource: jsx_import_source,
          },
        },
      })

      build.onResolve({ filter: CSS_QUERY_PATTERN }, (args) => ({
        path: args.path,
      }))

      build.onLoad({ filter: CSS_QUERY_PATTERN }, (args) => ({
        contents: css_cache.get(args.path) ?? "",
        loader: "css",
      }))

      build.onLoad(
        { filter: options.include ?? DEFAULT_INCLUDE, namespace: "file" },
        async (args) => {
          if (!should_compile(options, args.path)) return undefined

          const source = await readFile(args.path, "utf-8")
          const { code, css } = compile(source, args.path, compile_options)
          const css_id = to_css_id(args.path)

          let output = code
          if (emit_css && css) {
            css_cache.set(css_id, css.code)
            output = `import ${JSON.stringify(css_id)};\n${code}`
          } else {
            css_cache.delete(css_id)
          }

          return {
            contents: transpiler.transformSync(output),
            loader: "js",
          }
        },
      )
    },
  } satisfies BunPlugin
}

export default tsrxPreact
