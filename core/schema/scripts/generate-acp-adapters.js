import fs from "fs"
import path from "path"
import { fileURLToPath } from "url"

const __dirname = path.dirname(fileURLToPath(import.meta.url))

const REGISTRY_JSON_URL = "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json"

async function fetchAdapterIds() {
  const response = await fetch(REGISTRY_JSON_URL)

  if (!response.ok) {
    const errorBody = await response.text()
    throw new Error(`Failed to fetch registry contents: ${response.statusText}\n${errorBody}`)
  }

  const data = await response.json()
  if (!data || !Array.isArray(data.agents)) {
    throw new Error("Unexpected response format from registry.json")
  }
  return data.agents.map((agent) => agent.id)
}

async function main() {
  try {
    const ids = await fetchAdapterIds()
    const sortedIds = ids.sort()
    const values = sortedIds.map((id) => `  "${id}",`).join("\n")

    const content = `/**
 * This file is auto-generated. Do not edit manually.
 */

export const ACPAdapterNames = [
${values}
] as const

export type ACPAdapterName = (typeof ACPAdapterNames)[number] | (string & {})
`

    const outputPath = path.resolve(__dirname, "../src/acp-adapters.ts")
    fs.writeFileSync(outputPath, content)
    console.log(`Successfully generated ${outputPath}`)
  } catch (error) {
    console.error("Error generating ACP adapters type:", error)
    process.exit(1)
  }
}

main()
