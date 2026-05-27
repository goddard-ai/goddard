import { readFileSync } from "node:fs"
import { join } from "node:path"
import { defineConfig } from "bumpp"

const packageJson = JSON.parse(readFileSync(join(process.cwd(), "package.json"), "utf8")) as {
  name?: string
}
const packageName = packageJson.name?.replace(/^@goddard-ai\//, "")

if (!packageName) {
  throw new Error("bumpp requires the current package.json to define a package name.")
}

/**
 * Shared release configuration for bumpp.
 */
export default defineConfig({
  tag: `${packageName}@`,
})
