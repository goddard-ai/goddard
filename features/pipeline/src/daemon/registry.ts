import { loadProjectPipelineDefinitions } from "../loader.ts"
import type {
  PipelineDefinition,
  PipelineDefinitionDiagnostic,
  PipelineDefinitionSource,
  RegisteredPipelineDefinition,
} from "../schema.ts"

export type CodePipelineDefinitionRegistration = {
  source: Extract<PipelineDefinitionSource, "code" | "plugin" | "user">
  path?: string
  definition: PipelineDefinition
}

export type PipelineDefinitionRegistry = {
  list: (input: { cwd: string }) => Promise<{
    definitions: RegisteredPipelineDefinition[]
    diagnostics: PipelineDefinitionDiagnostic[]
  }>
  diagnostics: (input: { cwd: string }) => Promise<{
    diagnostics: PipelineDefinitionDiagnostic[]
  }>
}

export function createPipelineDefinitionRegistry(
  input: {
    definitions?: readonly CodePipelineDefinitionRegistration[]
  } = {},
): PipelineDefinitionRegistry {
  const staticDefinitions = input.definitions ?? []
  const list = async ({ cwd }: { cwd: string }) => {
    const loaded = await loadProjectPipelineDefinitions(cwd)
    const mergedDefinitions = mergeDefinitions([
      ...staticDefinitions.map((definition) => ({
        source: definition.source,
        path: definition.path,
        definition: definition.definition,
      })),
      ...loaded.definitions,
    ])

    return {
      definitions: mergedDefinitions.definitions,
      diagnostics: [...loaded.diagnostics, ...mergedDefinitions.diagnostics].sort(
        compareDiagnostics,
      ),
    }
  }

  return {
    list,
    async diagnostics({ cwd }) {
      const result = await list({ cwd })

      return {
        diagnostics: result.diagnostics,
      }
    },
  }
}

function mergeDefinitions(definitions: readonly RegisteredPipelineDefinition[]) {
  const seenKeys = new Set<string>()
  const merged: RegisteredPipelineDefinition[] = []
  const diagnostics: PipelineDefinitionDiagnostic[] = []

  for (const definition of sortDefinitions(definitions)) {
    const key = getDefinitionKey(definition.definition)

    if (seenKeys.has(key)) {
      diagnostics.push({
        source: definition.source,
        path: definition.path,
        message: `Duplicate Pipeline definition "${key}".`,
      })
      continue
    }

    seenKeys.add(key)
    merged.push(definition)
  }

  return {
    definitions: merged,
    diagnostics,
  }
}

function sortDefinitions(definitions: readonly RegisteredPipelineDefinition[]) {
  return [...definitions].sort((left, right) => {
    const idComparison = left.definition.id.localeCompare(right.definition.id)
    if (idComparison !== 0) {
      return idComparison
    }

    const versionComparison = left.definition.version.localeCompare(right.definition.version)
    if (versionComparison !== 0) {
      return versionComparison
    }

    const sourceComparison = left.source.localeCompare(right.source)
    if (sourceComparison !== 0) {
      return sourceComparison
    }

    return (left.path ?? "").localeCompare(right.path ?? "")
  })
}

function compareDiagnostics(
  left: PipelineDefinitionDiagnostic,
  right: PipelineDefinitionDiagnostic,
) {
  const sourceComparison = left.source.localeCompare(right.source)
  if (sourceComparison !== 0) {
    return sourceComparison
  }

  const pathComparison = (left.path ?? "").localeCompare(right.path ?? "")
  if (pathComparison !== 0) {
    return pathComparison
  }

  return left.message.localeCompare(right.message)
}

function getDefinitionKey(definition: PipelineDefinition) {
  return `${definition.id}@${definition.version}`
}
