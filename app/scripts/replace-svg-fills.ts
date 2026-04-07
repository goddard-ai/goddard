#!/usr/bin/env bun

import { readdir, readFile, stat, writeFile } from "node:fs/promises"
import path from "node:path"
import { fileURLToPath } from "node:url"

const scriptDirPath = path.dirname(fileURLToPath(import.meta.url))
const appRootPath = path.resolve(scriptDirPath, "..")
const fillPattern = /fill="(?!none|currentColor)[^"]*"/g

/** Rewrites non-transparent SVG fills to currentColor for every SVG in a target assets folder. */
async function main() {
  const targetPath = resolveTargetPath(process.argv[2])
  const assetsDirPath = path.join(targetPath, "assets")
  const assetPaths = await collectSvgAssetPaths(assetsDirPath)

  if (assetPaths.length === 0) {
    throw new Error(`Found no SVG assets in ${assetsDirPath}.`)
  }

  const changedAssets: string[] = []

  for (const assetPath of assetPaths) {
    const currentSource = await readFile(assetPath, "utf8")
    const nextSource = currentSource.replaceAll(fillPattern, 'fill="currentColor"')

    if (nextSource === currentSource) {
      continue
    }

    await writeFile(assetPath, nextSource)
    changedAssets.push(path.relative(targetPath, assetPath))
  }

  if (changedAssets.length === 0) {
    console.log(`No SVG fills needed changes in ${path.relative(appRootPath, targetPath)}.`)
    return
  }

  console.log(`Updated SVG fills in ${path.relative(appRootPath, targetPath)}:`)

  for (const assetPath of changedAssets) {
    console.log(`- ${assetPath}`)
  }
}

/** Resolves the requested target folder relative to the current working directory by default. */
function resolveTargetPath(targetArg: string | undefined) {
  if (!targetArg) {
    throw new Error(
      "Usage: bun ./scripts/replace-svg-fills.ts <target-folder>\n" +
        "Example: bun ./scripts/replace-svg-fills.ts webstudio-components/AppShell",
    )
  }

  return path.resolve(process.cwd(), targetArg)
}

/** Returns every SVG file found under the target folder's assets directory. */
async function collectSvgAssetPaths(assetsDirPath: string) {
  const assetsStat = await stat(assetsDirPath).catch(() => null)

  if (!assetsStat?.isDirectory()) {
    throw new Error(`Expected an assets directory at ${assetsDirPath}.`)
  }

  const assetPaths: string[] = []
  await walkSvgFiles(assetsDirPath, assetPaths)

  return assetPaths.sort((left, right) => left.localeCompare(right))
}

/** Recursively walks one directory tree and collects SVG file paths. */
async function walkSvgFiles(directoryPath: string, assetPaths: string[]) {
  const entries = await readdir(directoryPath, { withFileTypes: true })

  for (const entry of entries) {
    const entryPath = path.join(directoryPath, entry.name)

    if (entry.isDirectory()) {
      await walkSvgFiles(entryPath, assetPaths)
      continue
    }

    if (entry.isFile() && entry.name.endsWith(".svg")) {
      assetPaths.push(entryPath)
    }
  }
}

await main()
