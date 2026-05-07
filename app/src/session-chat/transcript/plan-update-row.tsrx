import { CheckCircle2, Circle, ListChecks, LoaderCircle } from "lucide-react"

import type { SessionTranscriptPlanEntry, SessionTranscriptPlanUpdate } from "~/sessions/models.ts"
import styles from "./transcript.style.ts"

function formatPlanEntryStatus(status: SessionTranscriptPlanEntry["status"]) {
  switch (status) {
    case "completed":
      return "Completed"
    case "in_progress":
      return "In progress"
    default:
      return "Pending"
  }
}

function formatPlanEntryPriority(priority: SessionTranscriptPlanEntry["priority"]) {
  return `${priority.slice(0, 1).toUpperCase()}${priority.slice(1)} priority`
}

function getPlanEntryStatusIcon(status: SessionTranscriptPlanEntry["status"]) {
  switch (status) {
    case "completed":
      return CheckCircle2
    case "in_progress":
      return LoaderCircle
    default:
      return Circle
  }
}

function buildPlanSummary(entries: readonly SessionTranscriptPlanEntry[]) {
  const completedCount = entries.filter((entry) => entry.status === "completed").length
  const inProgressCount = entries.filter((entry) => entry.status === "in_progress").length

  if (entries.length === 0) {
    return "No active steps"
  }

  return [
    `${entries.length} ${entries.length === 1 ? "step" : "steps"}`,
    `${completedCount} complete`,
    inProgressCount > 0 ? `${inProgressCount} in progress` : null,
  ].filter(Boolean).join(" · ")
}

/** Renders one complete ACP plan state update without exposing agent-thought content. */
export component PlanUpdateTranscriptRow(props: {
  message: SessionTranscriptPlanUpdate;
}) {
  <article class={styles.row}>
    <div class={styles.rowInner} style={{ justifyContent: "flex-start" }}>
      <section class={styles.planCard} aria-label={props.message.title}>
        <header class={styles.planHeader}>
          <span class={styles.planIcon} aria-hidden="true">
            <ListChecks size={16} strokeWidth={2.2} />
          </span>
          <div class={styles.planTitleGroup}>
            <h3 class={styles.planTitle}>{props.message.title}</h3>
            <div class={styles.planMeta}>
              {buildPlanSummary(props.message.entries)}
            </div>
          </div>
        </header>

        if (props.message.entries.length === 0) {
          <div class={styles.planEmpty}>{"The agent cleared its plan."}</div>
        } else {
          <ol class={styles.planEntryList}>
            for (const entry of props.message.entries; index index) {
              const StatusIcon = getPlanEntryStatusIcon(entry.status)

              <li
                class={styles.planEntryItem}
                key={`${entry.status}:${entry.priority}:${entry.content}:${index}`}
              >
                <span class={styles.planEntryStatus} data-status={entry.status} aria-hidden="true">
                  <StatusIcon size={13} strokeWidth={2.2} />
                </span>
                <div class={styles.planEntryContent}>
                  <div class={styles.planEntryText}>{entry.content}</div>
                  <div class={styles.planEntryMeta}>
                    <span>{formatPlanEntryStatus(entry.status)}</span>
                    <span>{formatPlanEntryPriority(entry.priority)}</span>
                  </div>
                </div>
              </li>
            }
          </ol>
        }
      </section>
    </div>
  </article>
}
