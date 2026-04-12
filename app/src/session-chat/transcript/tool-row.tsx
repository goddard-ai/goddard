import { cx } from "@goddard-ai/styled-system/css"
import type { MessageListRow } from "../message-list.tsx"
import type {
  SessionTranscriptToolCall,
  SessionTranscriptToolContent,
  SessionTranscriptToolStatus,
} from "~/sessions/models.ts"
import { buildToolDiffPreview, getBubbleMaxWidth } from "./layout.ts"
import { TranscriptMetaRow } from "./meta-row.tsx"
import {
  bubbleFrameClass,
  rowClass,
  rowColumnClass,
  rowInnerClass,
  toolBadgeClass,
  toolBadgeRowClass,
  toolBodyClass,
  toolBubbleClass,
  toolCardClass,
  toolCompletedBadgeClass,
  toolDiffClass,
  toolDiffPathClass,
  toolDiffPreviewClass,
  toolEmptyStateClass,
  toolFailedBadgeClass,
  toolHeaderClass,
  toolKindBadgeClass,
  toolLocationLineClass,
  toolLocationListClass,
  toolLocationPathClass,
  toolLocationRowClass,
  toolPendingBadgeClass,
  toolRunningBadgeClass,
  toolSectionClass,
  toolSectionLabelClass,
  toolTerminalClass,
  toolTerminalIdClass,
  toolTerminalLabelClass,
  toolTextClass,
  toolTitleClass,
} from "./styles.ts"

function getToolStatusBadgeClass(status: SessionTranscriptToolStatus) {
  if (status === "completed") {
    return toolCompletedBadgeClass
  }

  if (status === "failed") {
    return toolFailedBadgeClass
  }

  if (status === "in_progress") {
    return toolRunningBadgeClass
  }

  return toolPendingBadgeClass
}

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

function renderToolContent(content: SessionTranscriptToolContent, key: string) {
  if (content.type === "content") {
    return (
      <section key={key} class={toolSectionClass}>
        <span class={toolSectionLabelClass}>Output</span>
        <div class={toolTextClass}>{content.text?.trim() || "Structured tool output."}</div>
      </section>
    )
  }

  if (content.type === "diff") {
    return (
      <section key={key} class={toolSectionClass}>
        <span class={toolSectionLabelClass}>Diff</span>
        <div class={toolDiffClass}>
          {content.path ? <div class={toolDiffPathClass}>{content.path}</div> : null}
          <div class={toolDiffPreviewClass}>{buildToolDiffPreview(content)}</div>
        </div>
      </section>
    )
  }

  return (
    <section key={key} class={toolSectionClass}>
      <span class={toolSectionLabelClass}>Terminal</span>
      <div class={toolTerminalClass}>
        <span class={toolTerminalLabelClass}>Terminal Session</span>
        <span class={toolTerminalIdClass}>{content.terminalId}</span>
      </div>
    </section>
  )
}

function renderToolLocations(locations: SessionTranscriptToolCall["locations"]) {
  if (locations.length === 0) {
    return null
  }

  return (
    <section class={toolSectionClass}>
      <span class={toolSectionLabelClass}>Locations</span>
      <div class={toolLocationListClass}>
        {locations.map((location, index) => (
          <div
            key={`${location.path}:${location.line ?? "none"}:${index}`}
            class={toolLocationRowClass}
          >
            <span class={toolLocationPathClass}>{location.path}</span>
            {location.line != null ? (
              <span class={toolLocationLineClass}>Line {location.line}</span>
            ) : null}
          </div>
        ))}
      </div>
    </section>
  )
}

/** Renders one transcript tool row with dedicated status and payload sections. */
export function ToolTranscriptRow(props: { row: MessageListRow<SessionTranscriptToolCall> }) {
  const message = props.row.item
  const bubbleWidth = getBubbleMaxWidth(props.row.viewportWidth, message)

  return (
    <article class={rowClass}>
      <div class={rowInnerClass} style={{ justifyContent: "flex-start" }}>
        <div class={rowColumnClass} style={{ width: `${bubbleWidth}px` }}>
          <TranscriptMetaRow
            authorName={message.authorName}
            timestampLabel={message.timestampLabel}
            alignmentStyle={{ justifyContent: "flex-start" }}
          />
          <div class={cx(bubbleFrameClass, toolBubbleClass)}>
            <div class={toolCardClass}>
              <div class={toolHeaderClass}>
                <div class={toolTitleClass}>{message.title}</div>
                <div class={toolBadgeRowClass}>
                  <span class={cx(toolBadgeClass, toolKindBadgeClass)}>
                    {formatToolKindLabel(message.toolKind)}
                  </span>
                  <span class={cx(toolBadgeClass, getToolStatusBadgeClass(message.status))}>
                    {formatToolStatusLabel(message.status)}
                  </span>
                </div>
              </div>
              {message.content.length > 0 || message.locations.length > 0 ? (
                <div class={toolBodyClass}>
                  {message.content.map((content, index) =>
                    renderToolContent(content, `${message.id}:content:${index}`),
                  )}
                  {renderToolLocations(message.locations)}
                </div>
              ) : (
                <div class={toolEmptyStateClass}>No tool output was attached to this call.</div>
              )}
            </div>
          </div>
        </div>
      </div>
    </article>
  )
}
