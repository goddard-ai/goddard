import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { dirname, join } from "node:path"
import { afterEach, expect, test } from "bun:test"

import { loadProjectPipelineDefinitions } from "../src/loader.ts"

const cleanupDirs: string[] = []

afterEach(async () => {
  while (cleanupDirs.length > 0) {
    await rm(cleanupDirs.pop()!, { recursive: true, force: true })
  }
})

async function createRoot() {
  const root = await mkdtemp(join(tmpdir(), "goddard-pipeline-"))
  cleanupDirs.push(root)
  return root
}

async function writePipelineFile(root: string, path: string, content: string) {
  const filePath = join(root, ".goddard", "pipelines", path)
  await mkdir(dirname(filePath), { recursive: true })
  await writeFile(filePath, content, "utf-8")
  return filePath
}

async function writePrompt(root: string, path: string, content = "Write with care.") {
  const filePath = join(root, ".goddard", "pipelines", path)
  await mkdir(dirname(filePath), { recursive: true })
  await writeFile(filePath, content, "utf-8")
  return filePath
}

function pipelineYaml(input: { id?: string; promptFile?: string } = {}) {
  return `id: ${input.id ?? "creative-weaver"}
version: 0.1.0
name: Creative Weaver
inputs:
  premise:
    type: string
steps:
  - id: architect
    kind: agent
    name: Architect
    systemPromptFile: ${input.promptFile ?? "./prompts/architect.md"}
    input:
      premise: $.inputs.premise
  - id: chaos
    kind: script
    name: Chaos Weaver
    transformer: creative-weaver.sample-chaos
    input:
      ledger: $.steps.architect.output
`
}

test("loadProjectPipelineDefinitions loads directory pipeline definitions", async () => {
  const root = await createRoot()
  const promptPath = await writePrompt(root, "creative-weaver/prompts/architect.md")
  const definitionPath = await writePipelineFile(
    root,
    "creative-weaver/pipeline.yaml",
    pipelineYaml(),
  )

  const result = await loadProjectPipelineDefinitions(root)

  expect(result.diagnostics).toEqual([])
  expect(result.definitions).toHaveLength(1)
  expect(result.definitions[0]).toMatchObject({
    path: definitionPath,
    source: "project",
    definition: {
      id: "creative-weaver",
    },
  })
  expect(result.definitions[0]?.definition.steps[0]).toMatchObject({
    id: "architect",
    systemPromptFile: promptPath,
  })
})

test("loadProjectPipelineDefinitions loads single-file pipeline definitions", async () => {
  const root = await createRoot()
  const promptPath = await writePrompt(root, "prompts/architect.md")
  await writePipelineFile(root, "creative-weaver.yaml", pipelineYaml())

  const result = await loadProjectPipelineDefinitions(root)

  expect(result.diagnostics).toEqual([])
  expect(result.definitions[0]?.definition.steps[0]).toMatchObject({
    systemPromptFile: promptPath,
  })
})

test("loadProjectPipelineDefinitions reports missing prompt files", async () => {
  const root = await createRoot()
  const definitionPath = await writePipelineFile(
    root,
    "creative-weaver/pipeline.yaml",
    pipelineYaml(),
  )

  const result = await loadProjectPipelineDefinitions(root)

  expect(result.definitions).toEqual([])
  expect(result.diagnostics).toEqual([
    expect.objectContaining({
      path: definitionPath,
      message: expect.stringContaining("Prompt file not found:"),
    }),
  ])
})

test("loadProjectPipelineDefinitions reports duplicate ids", async () => {
  const root = await createRoot()
  await writePrompt(root, "creative-weaver/prompts/architect.md")
  await writePrompt(root, "duplicate/prompts/architect.md")
  await writePipelineFile(root, "creative-weaver/pipeline.yaml", pipelineYaml())
  const duplicatePath = await writePipelineFile(root, "duplicate/pipeline.yaml", pipelineYaml())

  const result = await loadProjectPipelineDefinitions(root)

  expect(result.definitions).toHaveLength(1)
  expect(result.diagnostics).toEqual([
    {
      path: duplicatePath,
      message: 'Duplicate Pipeline definition id "creative-weaver".',
    },
  ])
})

test("loadProjectPipelineDefinitions reports invalid schema", async () => {
  const root = await createRoot()
  const definitionPath = await writePipelineFile(
    root,
    "invalid/pipeline.yaml",
    `id: invalid
version: 0.1.0
inputs: {}
steps: []
`,
  )

  const result = await loadProjectPipelineDefinitions(root)

  expect(result.definitions).toEqual([])
  expect(result.diagnostics).toEqual(
    expect.arrayContaining([
      expect.objectContaining({
        path: definitionPath,
        message: expect.stringContaining("name:"),
      }),
      expect.objectContaining({
        path: definitionPath,
        message: expect.stringContaining("steps:"),
      }),
    ]),
  )
})

test("loadProjectPipelineDefinitions reports invalid YAML", async () => {
  const root = await createRoot()
  const definitionPath = await writePipelineFile(root, "broken.yaml", "id: [")

  const result = await loadProjectPipelineDefinitions(root)

  expect(result.definitions).toEqual([])
  expect(result.diagnostics).toEqual([
    expect.objectContaining({
      path: definitionPath,
      message: expect.stringContaining("Invalid YAML:"),
    }),
  ])
})

test("loadProjectPipelineDefinitions ignores missing pipeline roots", async () => {
  const root = await createRoot()

  await expect(loadProjectPipelineDefinitions(root)).resolves.toEqual({
    definitions: [],
    diagnostics: [],
  })
})
