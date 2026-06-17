import { useState } from "preact/hooks"

import { appToaster } from "~/lib/good-toaster.tsrx"
import { queryClient, useQuery } from "~/lib/query.ts"
import { goddardSdk } from "~/sdk.ts"
import styles from "./page.style.ts"

export type PipelineRunViewProps = {
  cwd?: string | null;
  pipelineName?: string;
  runId: string;
}

function formatJson(value: unknown) {
  return JSON.stringify(value, null, 2)
}

function getErrorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}

export default function PipelineRunView(props: PipelineRunViewProps) @{
  const [isBusy, setIsBusy] = useState(false)
  const result = useQuery(goddardSdk.pipeline.getRun, {
    params: [{ id: props.runId as `plr_${string}` }],
  })
  const { run, steps } = result

  async function runAction(label: string, action: () => Promise<unknown>) {
    setIsBusy(true)
    try {
      await action()
      queryClient.invalidate(goddardSdk.pipeline.getRun, [{ id: props.runId as `plr_${string}` }])
      queryClient.invalidate(goddardSdk.pipeline.listRuns)
    } catch (error) {
      const message = getErrorMessage(error)
      appToaster.create({
        title: `${label} failed`,
        description: message,
        type: "error",
      })
    } finally {
      setIsBusy(false)
    }
  }

  <div class={styles.root}>
    <header class={styles.runHeader}>
      <div class={styles.titleGroup}>
        <h1 class={styles.title}>
          {props.pipelineName ?? run.definitionSnapshot.definition.name}
        </h1>
        <p class={styles.subtitle}>{run.id} · {run.status} · {run.cwd}</p>
      </div>
      <div class={styles.actionRow}>
        <button
          class={styles.button}
          disabled={isBusy || run.status === "succeeded" || run.status === "cancelled"}
          type="button"
          onClick={() => runAction("Advance", () => goddardSdk.pipeline.advanceRun({ id: run.id }))}
        >Advance</button>
        <button
          class={styles.button}
          disabled={isBusy || run.status !== "waiting"}
          type="button"
          onClick={() => runAction("Approve", () => goddardSdk.pipeline.approveRun({ id: run.id }))}
        >Approve</button>
        <button
          class={styles.button}
          disabled={isBusy || run.status !== "failed"}
          type="button"
          onClick={() => runAction("Retry", () => goddardSdk.pipeline.retryRun({ id: run.id }))}
        >Retry</button>
        <button
          class={styles.button}
          disabled={isBusy || run.status === "succeeded" || run.status === "cancelled"}
          type="button"
          onClick={() => runAction("Cancel", () => goddardSdk.pipeline.cancelRun({ id: run.id }))}
        >Cancel</button>
      </div>
    </header>

    <main class={styles.runBody}>
      <section class={styles.stepList}>
        @for (const step of steps) {
          <article class={styles.step} key={step.id}>
            <header class={styles.stepHeader}>
              <div class={styles.stepTitle}>
                <span>
                  {step.stepIndex + 1}
                  .
                </span>
                <span>{step.stepId}</span>
                <span class={styles.itemMeta}>{step.kind}</span>
              </div>
              <span class={styles.badge}>{step.status}</span>
            </header>

            @if (step.sessionId) {
              <div class={styles.details}>
                <span class={styles.itemMeta}>Session {step.sessionId}</span>

                @if (step.stopReason) {
                  <span class={styles.itemMeta}> · {step.stopReason}</span>
                }
              </div>
            }
            @if (step.error) {
              <div class={styles.details}>
                <div class={styles.error}>{step.error}</div>
              </div>
            }

            <details class={styles.details}>
              <summary class={styles.summary}>Input</summary>
              <pre class={styles.code}>{formatJson(step.input)}</pre>
            </details>
            <details class={styles.details}>
              <summary class={styles.summary}>Output</summary>
              <pre class={styles.code}>{formatJson(step.output)}</pre>
            </details>
          </article>
        }
      </section>
    </main>
  </div>
}
