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

function renderMetaParts(parts: readonly string[], keyPrefix: string) {
  return parts.flatMap((part, index) => {
    const partNode = <tsx>
      <span key={`${keyPrefix}:part:${index}`}>
        {part}
      </span>
    </tsx>

    if (index === 0) {
      return [partNode]
    }

    return [
      <tsx>
        <span key={`${keyPrefix}:separator:${index}`} aria-hidden="true" class={toolInlineSeparatorClass} />
      </tsx>,
      partNode,
    ]
  })
}

function renderToolContent(content: SessionTranscriptToolContent, key: string) {
  if (content.type === "content") {
    return <tsx>
      <div key={key} class={toolDetailTextClass}>
        {content.text?.trim() || "Structured tool output."}
      </div>
    </tsx>
  }

  if (content.type === "diff") {
    return <tsx>
      <div key={key} class={toolDetailListClass}>
        {content.path
          ? <tsx>
              <div class={toolDetailMonoClass}>
                {content.path}
              </div>
            </tsx>
          : null}
        <div class={toolDetailMonoClass}>
          {buildToolDiffPreview(content)}
        </div>
      </div>
    </tsx>
  }

  return <tsx>
    <div key={key} class={toolDetailMonoClass}>
      {"Terminal session "}
      {content.terminalId}
    </div>
  </tsx>
}

function renderToolLocations(locations: SessionTranscriptToolCall["locations"]) {
  if (locations.length === 0) {
    return null
  }

  return <tsx>
    <div class={toolDetailListClass}>
      {locations.map(
        (location, index) => <tsx>
          <div key={`${location.path}:${
            location.line ?? "none"
          }:${index}`} class={toolDetailMonoClass}>
            {location.path}
            {location.line != null ? `:${location.line}` : ""}
          </div>
        </tsx>,
      )}
    </div>
  </tsx>
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
                {renderMetaParts(summaryMetaParts, `${props.groupId}:summary`)}
              </span>
            </span>
            <span class={toolToggleHintClass}>{"Details"}</span>
          </summary>
          <ol class={toolDetailListClass}>
            {props.toolCalls.map(
              (toolCall) => <tsx>
                <li key={`${props.groupId}:${toolCall.id}`} class={toolDetailItemClass}>
                  <article>
                    <div class={toolDetailHeadingClass}>
                      <span class={toolInlineMetaClass}>
                        {renderMetaParts(
                          [
                            toolCall.title,
                            formatToolKindLabel(toolCall.toolKind),
                            formatToolStatusLabel(toolCall.status),
                          ],
                          `${props.groupId}:${toolCall.id}:heading`,
                        )}
                      </span>
                    </div>
                    {toolCall.content.map(
                      (content, index) => renderToolContent(
                        content,
                        `${toolCall.id}:content:${index}`,
                      ),
                    )}
                    {renderToolLocations(toolCall.locations)}
                    {toolCall.content.length === 0 && toolCall.locations.length === 0
                      ? <tsx>
                          <div class={toolDetailTextClass}>
                            {"No tool output was attached to this call."}
                          </div>
                        </tsx>
                      : null}
                  </article>
                </li>
              </tsx>,
            )}
          </ol>
        </details>
      </div>
    </div>
  </article>
}
