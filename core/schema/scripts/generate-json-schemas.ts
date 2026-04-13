import fs from "fs/promises"
import path from "path"

import { buildGeneratedSchemaArtifacts } from "../src/json-schemas.ts"

async function main() {
  const jsonDir = path.resolve(process.cwd(), "json")
  await fs.mkdir(jsonDir, { recursive: true })

  for (const { name, jsonSchema } of buildGeneratedSchemaArtifacts()) {
    const outputPath = path.resolve(jsonDir, name)
    await fs.writeFile(outputPath, JSON.stringify(jsonSchema, null, 2))
    console.log(`Generated ${name} schema at ${outputPath}`)
  }
}

main().catch(console.error)
