import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { dirname, join } from "node:path"
import { afterEach, expect, test } from "bun:test"

import { createPipelineDefinitionRegistry } from "../src/daemon/registry.ts"
import type { PipelineDefinition } from "../src/schema.ts"

const cleanupDirs: string[] = []

afterEach(async () => {
  while (cleanupDirs.length > 0) {
    await rm(cleanupDirs.pop()!, { recursive: true, force: true })
  }
})

async function createRoot() {
  const root = await mkdtemp(join(tmpdir(), "goddard-pipeline-registry-"))
  cleanupDirs.push(root)
  return root
}

async function writePipelineFile(root: string, path: string, content: string) {
  const filePath = join(root, ".goddard", "pipelines", path)
  await mkdir(dirname(filePath), { recursive: true })
  await writeFile(filePath, content, "utf-8")
  return filePath
}

function definition(id: string, version: string): PipelineDefinition {
  return {
    id,
    version,
    name: id,
    inputs: {},
    steps: [
      {
        id: "draft",
        kind: "script",
        name: "Draft",
        transformer: "pipeline.test",
        input: {},
      },
    ],
  }
}

function pipelineYaml(id: string, version: string) {
  return `id: ${id}
version: ${version}
name: ${id}
inputs: {}
steps:
  - id: draft
    kind: script
    name: Draft
    transformer: pipeline.test
`
}

test("registry lists code and project definitions in stable order", async () => {
  const root = await createRoot()
  await writePipelineFile(root, "beta.yaml", pipelineYaml("beta", "0.1.0"))

  const registry = createPipelineDefinitionRegistry({
    definitions: [
      {
        source: "code",
        definition: definition("alpha", "0.1.0"),
      },
    ],
  })

  const result = await registry.list({ cwd: root })

  expect(result.diagnostics).toEqual([])
  expect(
    result.definitions.map((item) => `${item.definition.id}@${item.definition.version}`),
  ).toEqual(["alpha@0.1.0", "beta@0.1.0"])
  expect(result.definitions.map((item) => item.source)).toEqual(["code", "project"])
})

test("registry allows multiple versions of the same pipeline id", async () => {
  const root = await createRoot()
  await writePipelineFile(root, "creative.yaml", pipelineYaml("creative-weaver", "0.2.0"))
  const registry = createPipelineDefinitionRegistry({
    definitions: [
      {
        source: "code",
        definition: definition("creative-weaver", "0.1.0"),
      },
    ],
  })

  const result = await registry.list({ cwd: root })

  expect(result.diagnostics).toEqual([])
  expect(result.definitions.map((item) => item.definition.version)).toEqual(["0.1.0", "0.2.0"])
})

test("registry reports duplicate id and version registrations", async () => {
  const root = await createRoot()
  await writePipelineFile(root, "creative.yaml", pipelineYaml("creative-weaver", "0.1.0"))
  const registry = createPipelineDefinitionRegistry({
    definitions: [
      {
        source: "code",
        definition: definition("creative-weaver", "0.1.0"),
      },
    ],
  })

  const result = await registry.list({ cwd: root })

  expect(result.definitions).toHaveLength(1)
  expect(result.diagnostics).toEqual([
    {
      source: "project",
      path: expect.stringContaining("creative.yaml"),
      message: 'Duplicate Pipeline definition "creative-weaver@0.1.0".',
    },
  ])
})

test("registry exposes invalid project definition diagnostics", async () => {
  const root = await createRoot()
  const definitionPath = await writePipelineFile(
    root,
    "invalid.yaml",
    `id: invalid
version: 0.1.0
inputs: {}
steps: []
`,
  )
  const registry = createPipelineDefinitionRegistry()

  const result = await registry.diagnostics({ cwd: root })

  expect(result.diagnostics).toEqual(
    expect.arrayContaining([
      expect.objectContaining({
        source: "project",
        path: definitionPath,
        message: expect.stringContaining("name:"),
      }),
      expect.objectContaining({
        source: "project",
        path: definitionPath,
        message: expect.stringContaining("steps:"),
      }),
    ]),
  )
})
