import fs from "fs/promises"
import path from "path"
import { zodToJsonSchema } from "zod-to-json-schema"
import { RootConfig, ActionConfig, LoopConfig } from "../src/config.js"

async function main() {
  const jsonDir = path.resolve(process.cwd(), "json")
  await fs.mkdir(jsonDir, { recursive: true })

  const schemas = [
    { name: "goddard.json", schema: RootConfig },
    { name: "action.json", schema: ActionConfig },
    { name: "loop.json", schema: LoopConfig },
  ]

  for (const { name, schema } of schemas) {
    const jsonSchema = zodToJsonSchema(schema, name)
    const outputPath = path.resolve(jsonDir, name)
    await fs.writeFile(outputPath, JSON.stringify(jsonSchema, null, 2))
    console.log(`Generated ${name} schema at ${outputPath}`)
  }
}

main().catch(console.error)
