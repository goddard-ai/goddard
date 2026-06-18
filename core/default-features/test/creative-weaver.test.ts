import { dirname, resolve } from "node:path"
import { fileURLToPath } from "node:url"
import { creativeWeaverScriptTransformers } from "@goddard-ai/creative-weaver/pipeline"
import { loadProjectPipelineDefinitions } from "@goddard-ai/pipeline/loader"
import { expect, test } from "bun:test"

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../../..")
const creativeWeaverRoot = resolve(repoRoot, "playground/creative-weaver")

test("loads the checked-in Creative Weaver pipeline definition", async () => {
  const result = await loadProjectPipelineDefinitions(creativeWeaverRoot)

  expect(result.diagnostics).toEqual([])
  expect(result.definitions).toHaveLength(1)
  expect(result.definitions[0]?.definition).toMatchObject({
    id: "creative-weaver",
    version: "0.1.0",
    steps: [
      { id: "architect", kind: "agent" },
      { id: "chaos-weaver", kind: "script", transformer: "creative-weaver.build-payload" },
      { id: "artisan", kind: "agent" },
      { id: "editor", kind: "agent" },
    ],
  })
})

test("Creative Weaver chaos transformer is deterministic by seed", () => {
  const transformer = creativeWeaverScriptTransformers["creative-weaver.build-payload"]
  const input = {
    premise: "Two cartographers argue in a power outage.",
    emotion: "tension",
    seed: 11,
    targetWords: 500,
  }

  const first = transformer({ input })
  const second = transformer({ input })
  const differentSeed = transformer({
    input: {
      ...input,
      seed: 12,
    },
  })

  expect(second).toEqual(first)
  expect(differentSeed).not.toEqual(first)
})
