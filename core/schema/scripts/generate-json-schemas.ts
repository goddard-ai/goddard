import fs from "fs/promises"
import path from "path"
import { zodToJsonSchema } from "zod-to-json-schema"
import { RootConfig, ActionConfig, LoopConfig } from "../src/config.js"

async function main() {
  const schemasDir = path.resolve(process.cwd(), "schemas")
  await fs.mkdir(schemasDir, { recursive: true })

  const schemas = [
    { name: "goddard.json", schema: RootConfig },
    { name: "action.json", schema: ActionConfig },
    { name: "loop.json", schema: LoopConfig },
  ]

  for (const { name, schema } of schemas) {
    const jsonSchema = zodToJsonSchema(schema, name)
    const outputPath = path.resolve(schemasDir, name)
    await fs.writeFile(outputPath, JSON.stringify(jsonSchema, null, 2))
    console.log(`Generated ${name} schema at ${outputPath}`)
  }
}

main().catch(console.error)
