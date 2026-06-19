import { defineConfig } from "@lingui/cli"
import { createBabelExtractor, extractFromFileWithBabel } from "@lingui/cli/api/extractors/babel"
import type { ExtractorType } from "@lingui/conf"
import { compile } from "@tsrx/preact"

const tsrxExtractor: ExtractorType = {
  match(filename) {
    return filename.endsWith(".tsrx")
  },
  extract(filename, code, onMessageExtracted, ctx) {
    const result = compile(code, filename)
    return extractFromFileWithBabel(
      filename,
      result.code,
      onMessageExtracted,
      { ...ctx, sourceMaps: result.map },
      { plugins: ["typescript", "jsx"] },
    )
  },
}

export default defineConfig({
  sourceLocale: "en",
  locales: ["en"],
  extractors: [createBabelExtractor(), tsrxExtractor],
  catalogs: [
    {
      path: "<rootDir>/locales/{locale}/messages",
      include: ["<rootDir>/src"],
      exclude: ["<rootDir>/src/styled-system"],
    },
  ],
})
