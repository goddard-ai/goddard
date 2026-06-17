import { readdir, readFile, stat } from "node:fs/promises"
import { dirname, join, resolve } from "node:path"
import { parse } from "yaml"

import { PipelineDefinition, type PipelineStepDefinition } from "./schema.ts"

export type PipelineDefinitionDiagnostic = {
  path: string
  message: string
}

export type LoadedPipelineDefinition = {
  path: string
  source: "project"
  definition: PipelineDefinition
}

export type LoadProjectPipelineDefinitionsResult = {
  definitions: LoadedPipelineDefinition[]
  diagnostics: PipelineDefinitionDiagnostic[]
}

type CandidateDefinitionFile = {
  path: string
}

const pipelinesDirectory = ".goddard/pipelines"

export async function loadProjectPipelineDefinitions(rootDir: string) {
  const directory = join(rootDir, pipelinesDirectory)
  const candidates = await findProjectPipelineDefinitionFiles(directory)
  const diagnostics: PipelineDefinitionDiagnostic[] = []
  const definitions: LoadedPipelineDefinition[] = []
  const seenDefinitionIds = new Set<string>()

  for (const candidate of candidates) {
    const loaded = await loadPipelineDefinitionFile(candidate.path)

    diagnostics.push(...loaded.diagnostics)

    if (!loaded.definition) {
      continue
    }

    if (seenDefinitionIds.has(loaded.definition.id)) {
      diagnostics.push({
        path: candidate.path,
        message: `Duplicate Pipeline definition id "${loaded.definition.id}".`,
      })
      continue
    }

    seenDefinitionIds.add(loaded.definition.id)
    definitions.push({
      path: candidate.path,
      source: "project",
      definition: loaded.definition,
    })
  }

  return {
    definitions,
    diagnostics,
  } satisfies LoadProjectPipelineDefinitionsResult
}

async function findProjectPipelineDefinitionFiles(directory: string) {
  const entries = await readDirectoryEntries(directory)
  const candidates: CandidateDefinitionFile[] = []

  for (const entry of entries) {
    if (entry.isFile() && entry.name.endsWith(".yaml")) {
      candidates.push({ path: join(directory, entry.name) })
      continue
    }

    if (entry.isDirectory()) {
      const pipelinePath = join(directory, entry.name, "pipeline.yaml")
      if (await isFile(pipelinePath)) {
        candidates.push({ path: pipelinePath })
      }
    }
  }

  return candidates.sort((left, right) => left.path.localeCompare(right.path))
}

async function readDirectoryEntries(directory: string) {
  try {
    return await readdir(directory, { withFileTypes: true })
  } catch (error) {
    if (isNotFoundError(error)) {
      return []
    }

    throw error
  }
}

async function loadPipelineDefinitionFile(path: string) {
  const diagnostics: PipelineDefinitionDiagnostic[] = []
  let raw: string

  try {
    raw = await readFile(path, "utf-8")
  } catch (error) {
    if (isNotFoundError(error)) {
      return {
        definition: null,
        diagnostics: [
          {
            path,
            message: "Pipeline definition file not found.",
          },
        ],
      }
    }

    throw error
  }

  let parsed: unknown

  try {
    parsed = parse(raw)
  } catch (error) {
    diagnostics.push({
      path,
      message: `Invalid YAML: ${getErrorMessage(error)}`,
    })
    return { definition: null, diagnostics }
  }

  const normalized = normalizePromptFilePaths(parsed, path)
  const result = PipelineDefinition.safeParse(normalized)

  if (!result.success) {
    diagnostics.push(
      ...result.error.issues.map((issue) => ({
        path,
        message:
          issue.path.length > 0 ? `${issue.path.join(".")}: ${issue.message}` : issue.message,
      })),
    )
    return { definition: null, diagnostics }
  }

  const missingPromptFiles = await findMissingPromptFiles(result.data, path)
  if (missingPromptFiles.length > 0) {
    diagnostics.push(
      ...missingPromptFiles.map((promptPath) => ({
        path,
        message: `Prompt file not found: ${promptPath}`,
      })),
    )
    return { definition: null, diagnostics }
  }

  return { definition: result.data, diagnostics }
}

function normalizePromptFilePaths(value: unknown, definitionPath: string) {
  const definitionDir = dirname(definitionPath)
  const parsed = PipelineDefinition.safeParse(value)
  if (!parsed.success) {
    return value
  }

  return {
    ...parsed.data,
    steps: parsed.data.steps.map((step) => normalizeStepPromptFilePath(step, definitionDir)),
  }
}

function normalizeStepPromptFilePath(step: PipelineStepDefinition, definitionDir: string) {
  if (step.kind !== "agent" || !step.systemPromptFile) {
    return step
  }

  return {
    ...step,
    systemPromptFile: resolve(definitionDir, step.systemPromptFile),
  }
}

async function findMissingPromptFiles(definition: PipelineDefinition, definitionPath: string) {
  const definitionDir = dirname(definitionPath)
  const missing: string[] = []

  for (const step of definition.steps) {
    if (step.kind !== "agent" || !step.systemPromptFile) {
      continue
    }

    const promptPath = resolve(definitionDir, step.systemPromptFile)
    try {
      const promptStat = await stat(promptPath)
      if (!promptStat.isFile()) {
        missing.push(promptPath)
      }
    } catch (error) {
      if (isNotFoundError(error)) {
        missing.push(promptPath)
        continue
      }

      throw error
    }
  }

  return missing
}

function isNotFoundError(error: unknown) {
  return typeof error === "object" && error !== null && "code" in error && error.code === "ENOENT"
}

async function isFile(path: string) {
  try {
    return (await stat(path)).isFile()
  } catch (error) {
    if (isNotFoundError(error)) {
      return false
    }

    throw error
  }
}

function getErrorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}
