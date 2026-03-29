import { execFileSync } from "node:child_process"
import { mkdir, readFile, writeFile } from "node:fs/promises"
import path from "node:path"
import { fileURLToPath } from "node:url"

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const subrepoRoot = path.resolve(__dirname, "../")
const outputDirectory = path.join(subrepoRoot, "docs/third_party")

const sources = [
  {
    packageName: "electrobun",
    sourceUrl: "https://blackboard.sh/electrobun/llms.txt",
  },
  {
    packageName: "preact-sigma",
    sourceUrl: "https://app.unpkg.com/preact-sigma@%5E2/files/llms.txt",
  },
]

/**
 * Convert viewer-oriented package CDN URLs into raw text download URLs.
 */
function toDownloadUrl(sourceUrl) {
  const url = new URL(sourceUrl)

  if (url.hostname === "app.unpkg.com") {
    url.hostname = "unpkg.com"
    url.pathname = url.pathname.replace("/files/", "/")
  }

  return url.toString()
}

/**
 * Read a file when present and otherwise report that it does not exist yet.
 */
async function readFileIfExists(filePath) {
  try {
    return await readFile(filePath, "utf8")
  } catch (error) {
    if (error?.code === "ENOENT") {
      return null
    }

    throw error
  }
}

/**
 * Fetch one third-party LLM reference and write it only when the contents changed.
 */
async function syncSource({ packageName, sourceUrl }) {
  const response = await fetch(toDownloadUrl(sourceUrl))

  if (!response.ok) {
    const errorBody = await response.text()
    throw new Error(
      `Failed to fetch ${packageName}: ${response.status} ${response.statusText}\n${errorBody}`,
    )
  }

  const contentType = response.headers.get("content-type") ?? ""

  if (contentType.includes("text/html")) {
    throw new Error(
      `Refusing to store HTML for ${packageName}; expected raw text from ${sourceUrl}`,
    )
  }

  const nextContent = await response.text()
  const outputPath = path.join(outputDirectory, `${packageName}.txt`)
  const currentContent = await readFileIfExists(outputPath)

  if (currentContent === nextContent) {
    return {
      outputPath,
      updated: false,
    }
  }

  await writeFile(outputPath, nextContent)

  return {
    outputPath,
    updated: true,
  }
}

/**
 * Return true when the generated files have staged, unstaged, or untracked changes.
 */
function hasTrackedChanges(pathsToCheck) {
  const status = execFileSync("git", ["status", "--short", "--", ...pathsToCheck], {
    cwd: subrepoRoot,
    encoding: "utf8",
  })

  return status.trim().length > 0
}

/**
 * Stage the generated files and create a docs-only commit when they changed.
 */
function commitChanges(pathsToCommit) {
  if (!hasTrackedChanges(pathsToCommit)) {
    console.log("No third-party docs changes to commit.")
    return
  }

  execFileSync("git", ["add", "--", ...pathsToCommit], {
    cwd: subrepoRoot,
    stdio: "inherit",
  })
  execFileSync(
    "git",
    [
      "commit",
      "-m",
      "docs(third-party): refresh llms snapshots",
      "-m",
      "- update stored llms snapshots for electrobun and preact-sigma\n- keep docs/third_party aligned with upstream package guidance",
    ],
    {
      cwd: subrepoRoot,
      stdio: "inherit",
    },
  )
}

/**
 * Synchronize all configured third-party docs and optionally commit the result.
 */
async function main() {
  await mkdir(outputDirectory, { recursive: true })

  const results = await Promise.all(sources.map(syncSource))
  const updatedPaths = results.filter((result) => result.updated).map((result) => result.outputPath)

  if (updatedPaths.length === 0) {
    console.log("Third-party docs are already up to date.")
  } else {
    for (const updatedPath of updatedPaths) {
      console.log(`Updated ${path.relative(subrepoRoot, updatedPath)}`)
    }
  }

  if (process.argv.includes("--commit")) {
    commitChanges(results.map((result) => result.outputPath))
  }
}

main().catch((error) => {
  console.error(error)
  process.exit(1)
})
