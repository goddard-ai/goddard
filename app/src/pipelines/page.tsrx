import { useEffect, useState } from "preact/hooks"
import type {
  ListPipelineDefinitionsResponse,
  RegisteredPipelineDefinition,
} from "@goddard-ai/pipeline/schema"

import { useProjectContext, useWorkbenchTabSet } from "~/app-state-context.tsrx"
import { appToaster } from "~/lib/good-toaster.tsrx"
import { queryClient, useQueries } from "~/lib/query.ts"
import { goddardSdk } from "~/sdk.ts"
import styles from "./page.style.ts"

function getDefinitionKey(item: RegisteredPipelineDefinition) {
  return `${item.definition.id}@${item.definition.version}`
}

function getDefaultInputJson(item: RegisteredPipelineDefinition | null) {
  if (!item) {
    return "{}"
  }

  return JSON.stringify(
    Object.fromEntries(Object.keys(item.definition.inputs).map((key) => [key, ""])),
    null,
    2,
  )
}

function getErrorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}

const emptyDefinitionResult = {
  definitions: [],
  diagnostics: [],
} satisfies ListPipelineDefinitionsResponse

export default function PipelinesPage() @{
  const projectContext = useProjectContext()
  const workbenchTabSet = useWorkbenchTabSet()
  const cwd = projectContext.activeProjectPath
  const [selectedKey, setSelectedKey] = useState<string | null>(null)
  const [inputJson, setInputJson] = useState("{}")
  const [spawnError, setSpawnError] = useState<string | null>(null)
  const [isSpawning, setIsSpawning] = useState(false)
  const listDefinitionsQuery: any = cwd ? goddardSdk.pipeline.listDefinitions : {}

  const [definitionResult, runResult] = useQueries(
    (query) => [
      query(
        listDefinitionsQuery,
        {
          params: cwd ? [{ cwd }] : null,
        } as any,
      ),
      query(goddardSdk.pipeline.listRuns, {
        params: [{ limit: 50 }],
      }),
    ],
  )
  const resolvedDefinitionResult =
    typeof definitionResult === "object" && definitionResult !== null &&
    "definitions" in definitionResult
      ? definitionResult as ListPipelineDefinitionsResponse
      : emptyDefinitionResult
  const definitions: RegisteredPipelineDefinition[] = resolvedDefinitionResult.definitions
  const diagnostics = resolvedDefinitionResult.diagnostics
  const selectedDefinition =
    definitions.find((item) => getDefinitionKey(item) === selectedKey) ?? definitions[0] ?? null

  useEffect(() => {
    if (!selectedDefinition) {
      return
    }

    const key = getDefinitionKey(selectedDefinition)
    if (selectedKey !== key) {
      setSelectedKey(key)
      setInputJson(getDefaultInputJson(selectedDefinition))
    }
  }, [selectedDefinition, selectedKey])

  async function spawnRun() {
    if (!cwd || !selectedDefinition) {
      return
    }

    setSpawnError(null)
    setIsSpawning(true)
    try {
      const parsedInputs = JSON.parse(inputJson) as Record<string, unknown>
      const result = await goddardSdk.pipeline.spawnRun({
        cwd,
        pipelineId: selectedDefinition.definition.id,
        pipelineVersion: selectedDefinition.definition.version,
        inputs: parsedInputs,
        origin: "app",
        visibility: "visible",
      })
      queryClient.invalidate(goddardSdk.pipeline.listRuns)
      workbenchTabSet.openOrFocusTab({
        kind: "pipelineRun",
        props: {
          cwd,
          pipelineName: selectedDefinition.definition.name,
          runId: result.run.id,
        },
      })
    } catch (error) {
      const message = getErrorMessage(error)
      setSpawnError(message)
      appToaster.create({ title: "Pipeline run failed", description: message, type: "error" })
    } finally {
      setIsSpawning(false)
    }
  }

  function openRun(runId: string) {
    const run = runResult.runs.find((item) => item.id === runId)
    workbenchTabSet.openOrFocusTab({
      kind: "pipelineRun",
      props: {
        cwd: run?.cwd ?? cwd,
        pipelineName: run?.definitionSnapshot.definition.name,
        runId,
      },
    })
  }

  <div class={styles.root}>
    <header class={styles.toolbar}>
      <div class={styles.titleGroup}>
        <h1 class={styles.title}>Pipelines</h1>
        <p class={styles.subtitle}>
          {cwd ?? "Select a project to load project pipelines."}
        </p>
      </div>
      <button
        class={styles.button}
        type="button"
        onClick={() => {
          if (cwd) {
            queryClient.invalidate(goddardSdk.pipeline.listDefinitions, [{ cwd }])
          }
          queryClient.invalidate(goddardSdk.pipeline.listRuns)
        }}
      >Refresh</button>
    </header>

    <div class={styles.body}>
      <aside class={styles.pane}>
        <h2 class={styles.paneTitle}>Definitions</h2>

        @if (!cwd) {
          <div class={styles.notice}>Open or select a project before spawning a pipeline run.</div>
        } @else if (definitions.length === 0) {
          <div class={styles.notice}>No pipeline definitions found for this project.</div>
        } @else {
          <div class={styles.definitionList}>
            @for (const item of definitions) {
              const key = getDefinitionKey(item)
              <button
                class={styles.definitionButton}
                data-selected={selectedDefinition === item}
                key={key}
                type="button"
                onClick={() => {
                  setSelectedKey(key)
                  setInputJson(getDefaultInputJson(item))
                }}
              >
                <span class={styles.itemTitle}>{item.definition.name}</span>
                <span
                  class={styles.itemMeta}
                >{item.definition.id} · {item.definition.version}</span>
                <span class={styles.itemMeta}>{item.source}</span>
              </button>
            }
          </div>
        }
      </aside>

      <main class={styles.workspace}>
        <section class={styles.section}>
          <h2 class={styles.paneTitle}>Spawn Run</h2>

          @if (selectedDefinition) {
            <>
              <textarea
                aria-label="Pipeline input JSON"
                class={styles.textArea}
                value={inputJson}
                onInput={(event) => {
                  setInputJson(event.currentTarget.value)
                }}
              />

              @if (spawnError) {
                <div class={styles.error}>{spawnError}</div>
              }

              <div class={styles.actionRow}>
                <button
                  class={`${styles.button} ${styles.primaryButton}`}
                  disabled={isSpawning || !cwd}
                  type="button"
                  onClick={spawnRun}
                >Spawn run</button>
              </div>
            </>
          } @else {
            <div class={styles.notice}>Choose a pipeline definition to prepare a run.</div>
          }

          @if (diagnostics.length > 0) {
            <>
              <h2 class={styles.paneTitle}>Diagnostics</h2>

              @for (const diagnostic of diagnostics) {
                <div
                  class={styles.notice}
                  key={`${diagnostic.path ?? diagnostic.source}:${diagnostic.message}`}
                >
                  {diagnostic.path ?? diagnostic.source}
                  :
                  {diagnostic.message}
                </div>
              }
            </>
          }

          <h2 class={styles.paneTitle}>Recent Runs</h2>

          @if (runResult.runs.length === 0) {
            <div class={styles.notice}>No pipeline runs yet.</div>
          } @else {
            <div class={styles.runList}>
              @for (const run of runResult.runs) {
                <button
                  class={styles.runButton}
                  key={run.id}
                  type="button"
                  onClick={() => openRun(run.id)}
                >
                  <span>
                    <span class={styles.itemTitle}>{run.definitionSnapshot.definition.name}</span>
                    <span class={styles.itemMeta}>{run.id}</span>
                  </span>
                  <span class={styles.badge}>{run.status}</span>
                </button>
              }
            </div>
          }
        </section>
      </main>
    </div>
  </div>
}
