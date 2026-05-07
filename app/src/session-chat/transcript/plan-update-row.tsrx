import { CheckCircle2, Circle, ListChecks, LoaderCircle } from "lucide-react"

import type { SessionTranscriptPlanEntry, SessionTranscriptPlanUpdate } from "~/sessions/models.ts"
import {
  planCardClass,
  planEmptyClass,
  planEntryContentClass,
  planEntryItemClass,
  planEntryListClass,
  planEntryMetaClass,
  planEntryStatusClass,
  planEntryTextClass,
  planHeaderClass,
  planIconClass,
  planMetaClass,
  planTitleClass,
  planTitleGroupClass,
  rowClass,
  rowInnerClass,
} from "./transcript.style.ts"

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
  <article class={rowClass}>
    <div class={rowInnerClass} style={{ justifyContent: "flex-start" }}>
      <section class={planCardClass} aria-label={props.message.title}>
        <header class={planHeaderClass}>
          <span class={planIconClass} aria-hidden="true">
            <ListChecks size={16} strokeWidth={2.2} />
          </span>
          <div class={planTitleGroupClass}>
            <h3 class={planTitleClass}>{props.message.title}</h3>
            <div class={planMetaClass}>
              {buildPlanSummary(props.message.entries)}
            </div>
          </div>
        </header>

        if (props.message.entries.length === 0) {
          <div class={planEmptyClass}>{"The agent cleared its plan."}</div>
        } else {
          <ol class={planEntryListClass}>
            for (const entry of props.message.entries; index index) {
              const StatusIcon = getPlanEntryStatusIcon(entry.status)

              <li
                class={planEntryItemClass}
                key={`${entry.status}:${entry.priority}:${entry.content}:${index}`}
              >
                <span class={planEntryStatusClass} data-status={entry.status} aria-hidden="true">
                  <StatusIcon size={13} strokeWidth={2.2} />
                </span>
                <div class={planEntryContentClass}>
                  <div class={planEntryTextClass}>{entry.content}</div>
                  <div class={planEntryMetaClass}>
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
