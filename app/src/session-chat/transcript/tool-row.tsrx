import type {
  SessionTranscriptToolCall,
  SessionTranscriptToolContent,
  SessionTranscriptToolStatus,
} from "~/sessions/models.ts"
import { buildToolDiffPreview } from "./layout.ts"
import {
  rowClass,
  rowColumnClass,
  rowInnerClass,
  toolDisclosureClass,
  toolDetailHeadingClass,
  toolInlineMetaClass,
  toolInlineSeparatorClass,
  toolDetailItemClass,
  toolDetailListClass,
  toolDetailMonoClass,
  toolDetailTextClass,
  toolSummaryClass,
  toolSummaryTextClass,
  toolToggleHintClass,
} from "./styles.ts"

function formatToolKindLabel(toolKind: SessionTranscriptToolCall["toolKind"]) {
  return toolKind === "switch_mode"
    ? "Switch mode"
    : `${toolKind.slice(0, 1).toUpperCase()}${toolKind.slice(1)}`
}

function formatToolStatusLabel(status: SessionTranscriptToolStatus) {
  return status === "in_progress"
    ? "Running"
    : `${status.slice(0, 1).toUpperCase()}${status.slice(1)}`
}

function buildToolGroupSummaryParts(toolCalls: readonly SessionTranscriptToolCall[]) {
  const visibleTitles = toolCalls.slice(0, 3).map((toolCall) => toolCall.title)
  const moreCount = Math.max(0, toolCalls.length - visibleTitles.length)

  return [
    toolCalls.length === 1 ? "Tool call" : `${toolCalls.length} tool calls`,
    visibleTitles.join(", "),
    moreCount > 0 ? `+${moreCount} more` : null,
  ].filter(Boolean) as string[]
}

function buildToolStatusSummary(toolCalls: readonly SessionTranscriptToolCall[]) {
  const counts: Record<SessionTranscriptToolStatus, number> = {
    pending: 0,
    in_progress: 0,
    completed: 0,
    failed: 0,
  }

  for (const toolCall of toolCalls) {
    counts[toolCall.status] += 1
  }

  return Object.entries(counts).filter(([, count]) => count > 0).map(
    ([status, count]) => `${count} ${formatToolStatusLabel(
      status as SessionTranscriptToolStatus,
    ).toLowerCase()}`,
  ).join(", ")
}

component ToolMetaParts(props: {
  parts: readonly string[];
  keyPrefix: string;
}) {
  for (const part of props.parts; index index) {
    if (index > 0) {
      <span
        key={`${props.keyPrefix}:separator:${index}`}
        aria-hidden="true"
        class={toolInlineSeparatorClass}
      />
    }

    <span key={`${props.keyPrefix}:part:${index}`}>
      {part}
    </span>
  }
}

component ToolContentDetail(props: {
  content: SessionTranscriptToolContent;
}) {
  if (props.content.type === "content") {
    <div class={toolDetailTextClass}>
      {props.content.text?.trim() || "Structured tool output."}
    </div>
  } else if (props.content.type === "diff") {
    <div class={toolDetailListClass}>
      if (props.content.path) {
        <div class={toolDetailMonoClass}>
          {props.content.path}
        </div>
      }

      <div class={toolDetailMonoClass}>
        {buildToolDiffPreview(props.content)}
      </div>
    </div>
  } else {
    <div class={toolDetailMonoClass}>
      {"Terminal session "}
      {props.content.terminalId}
    </div>
  }
}

component ToolLocationsList(props: {
  locations: SessionTranscriptToolCall["locations"];
}) {
  if (props.locations.length === 0) {
    return
  }

  <div class={toolDetailListClass}>
    for (const location of props.locations; index index) {
      <div class={toolDetailMonoClass} key={`${location.path}:${location.line ?? "none"}:${index}`}>
        {location.path}
        {location.line != null ? `:${location.line}` : ""}
      </div>
    }
  </div>
}

/** Renders one collapsed transcript tool row that expands into plain-text details on demand. */
export component ToolTranscriptRow(props: {
  groupId: string;
  toolCalls: readonly SessionTranscriptToolCall[];
}) {
  const summaryParts = buildToolGroupSummaryParts(props.toolCalls)
  const statusSummary = buildToolStatusSummary(props.toolCalls)
  const summaryMetaParts = statusSummary ? [...summaryParts, statusSummary] : summaryParts

  <article class={rowClass}>
    <div class={rowInnerClass} style={{ justifyContent: "flex-start" }}>
      <div class={rowColumnClass} style={{ width: "100%" }}>
        <details class={toolDisclosureClass}>
          <summary class={toolSummaryClass}>
            <span class={toolSummaryTextClass}>
              <span class={toolInlineMetaClass}>
                <ToolMetaParts parts={summaryMetaParts} keyPrefix={`${props.groupId}:summary`} />
              </span>
            </span>
            <span class={toolToggleHintClass}>{"Details"}</span>
          </summary>
          <ol class={toolDetailListClass}>
            for (const toolCall of props.toolCalls) {
              <li class={toolDetailItemClass} key={`${props.groupId}:${toolCall.id}`}>
                <article>
                  <div class={toolDetailHeadingClass}>
                    <span class={toolInlineMetaClass}>
                      <ToolMetaParts
                        parts={[
                          toolCall.title,
                          formatToolKindLabel(toolCall.toolKind),
                          formatToolStatusLabel(toolCall.status),
                        ]}
                        keyPrefix={`${props.groupId}:${toolCall.id}:heading`}
                      />
                    </span>
                  </div>

                  for (const content of toolCall.content; index index) {
                    <ToolContentDetail key={`${toolCall.id}:content:${index}`} {content} />
                  }

                  <ToolLocationsList locations={toolCall.locations} />
                  if (toolCall.content.length === 0 && toolCall.locations.length === 0) {
                    <div class={toolDetailTextClass}>
                      {"No tool output was attached to this call."}
                    </div>
                  }
                </article>
              </li>
            }
          </ol>
        </details>
      </div>
    </div>
  </article>
}
