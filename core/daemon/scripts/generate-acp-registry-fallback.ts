import { mkdtemp, readdir, readFile, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"

const registryUrl = "https://github.com/agentclientprotocol/registry"
const outputPath = new URL("../src/session/registry-fallback.ts", import.meta.url)

type FallbackAdapterEntry = {
  id: string
  name: string
  version: string
  description: string
  repository?: string
  website?: string
  authors?: string[]
  license?: string
  icon?: string
  distribution: Record<string, unknown>
  unofficial: boolean
  source: "registry"
}

async function runGit(cwd: string, args: string[]) {
  const result = Bun.spawn(["git", ...args], {
    cwd,
    stdin: "ignore",
    stdout: "pipe",
    stderr: "pipe",
  })

  const [stdout, stderr] = await Promise.all([
    result.stdout ? new Response(result.stdout).text() : "",
    result.stderr ? new Response(result.stderr).text() : "",
    result.exited,
  ])

  if (result.exitCode !== 0) {
    throw new Error(stderr.trim() || stdout.trim() || `git ${args.join(" ")} failed`)
  }
}

function renderModule(entries: unknown[]) {
  return `/**
 * This file is auto-generated. Do not edit manually.
 */

import type { AdapterCatalogEntry } from "@goddard-ai/schema/daemon"

export const ACPRegistryFallbackCatalog: AdapterCatalogEntry[] = ${JSON.stringify(entries, null, 2)}
`
}

async function readFallbackEntries(rootDir: string) {
  const entries = await readdir(rootDir, { withFileTypes: true })
  const adapters = await Promise.all(
    entries
      .filter((entry) => entry.isDirectory() && entry.name.startsWith(".") === false)
      .map(async (entry) => {
        const agentDir = join(rootDir, entry.name)
        const agent = JSON.parse(await readFile(join(agentDir, "agent.json"), "utf8")) as Record<
          string,
          unknown
        >
        let icon: string | undefined

        try {
          const svg = await readFile(join(agentDir, "icon.svg"), "utf8")
          icon = `data:image/svg+xml;base64,${Buffer.from(svg, "utf8").toString("base64")}`
        } catch {}

        return {
          ...agent,
          icon: icon ?? agent.icon,
          unofficial: entry.name.endsWith("-acp"),
          source: "registry",
        } as FallbackAdapterEntry
      }),
  )

  return adapters.sort((left, right) => {
    const leftName = typeof left.name === "string" ? left.name : left.id
    const rightName = typeof right.name === "string" ? right.name : right.id
    const nameCompare = String(leftName).localeCompare(String(rightName), undefined, {
      sensitivity: "base",
    })
    return nameCompare !== 0 ? nameCompare : String(left.id).localeCompare(String(right.id))
  })
}

async function main() {
  const tempDir = await mkdtemp(join(tmpdir(), "goddard-acp-registry-"))
  const cloneDir = join(tempDir, "registry")

  try {
    await runGit(tempDir, [
      "clone",
      "--depth",
      "1",
      "--single-branch",
      "--branch",
      "main",
      registryUrl,
      cloneDir,
    ])
    const entries = await readFallbackEntries(cloneDir)
    await writeFile(outputPath, renderModule(entries), "utf8")
  } finally {
    await rm(tempDir, { recursive: true, force: true })
  }
}

await main()
