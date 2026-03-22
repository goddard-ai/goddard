import fs from "fs/promises"
import path from "path"
import { toJSONSchema, z } from "zod"
import type { ToJSONSchemaParams } from "zod/v4/core"
import { ActionConfig, LoopConfig, UserConfig, registerConfigSchemas } from "../src/config.ts"

async function main() {
  const jsonDir = path.resolve(process.cwd(), "json")
  await fs.mkdir(jsonDir, { recursive: true })

  const acpRegistry = z.registry()
  registerConfigSchemas(acpRegistry)

  const schemas = [
    { name: "goddard.json", schema: UserConfig },
    { name: "action.json", schema: ActionConfig },
    { name: "loop.json", schema: LoopConfig },
  ]

  const schemaParams: ToJSONSchemaParams = {
    target: "draft-07",
    io: "input",
    override(ctx) {
      const { id } = ctx.jsonSchema
      if (id && acpRegistry.has(ctx.zodSchema)) {
        for (const key in ctx.jsonSchema) {
          delete ctx.jsonSchema[key]
        }
        ctx.jsonSchema.$ref = `https://raw.githubusercontent.com/agentclientprotocol/agent-client-protocol/main/schema/schema.json#/$defs/${id}`
      }
    },
  }

  for (const { name, schema } of schemas) {
    const jsonSchema = toJSONSchema(
      schema.extend({
        $schema: z.string(),
      }),
      schemaParams,
    )
    const outputPath = path.resolve(jsonDir, name)
    await fs.writeFile(outputPath, JSON.stringify(jsonSchema, null, 2))
    console.log(`Generated ${name} schema at ${outputPath}`)
  }
}

main().catch(console.error)
